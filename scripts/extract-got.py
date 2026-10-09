#!/usr/bin/env python3
"""Checks the GOT service catalogue against the fee schedule and writes corrections (T061, R9).

The source is the consolidated GOT the Federal Ministry of Justice publishes:

    curl -o requirements/GOT_2022.pdf https://www.gesetze-im-internet.de/got_2022/GOT.pdf

It is not committed — git ignores `requirements/GOT_2022.pdf` — because the fees are what we
need, not the typesetting. Its Gebührenverzeichnis lists one position per row:

    16         Allgemeine Untersuchung mit Beratung, Hund, Katze, Frettchen        23,62
    22         Folgeuntersuchung im selben Behandlungsfall mit Beratung, Pferd, Hausequiden,
               Kameliden                                                            24,62

so a position is a running number, a description that may wrap, and the single rate, with
thousands separated by a space ("1 350,00"). Only the single rate matters: the factor lives
on the treatment line (FR-021). Chapter headings are centred; page furniture repeats on every
page.

The catalogue was imported once, by migration `0008_got_import.sql`, and migrations only go
forward — so this script never writes a catalogue. It compares the schedule with the catalogue
a database holds and writes a migration that corrects the difference (that is how `0021` was
made):

    psql "$DATABASE_URL" -c "\\copy (SELECT got_number, name, net_price, hidden FROM service
         WHERE type = 'got' ORDER BY id) TO 'current.csv' WITH CSV HEADER"
    scripts/extract-got.py --against current.csv --out k-vet-backend/migrations/00NN_got_corrections.sql

Without `--against` it parses and validates only. Validation is the point: every number from 1
to the last must be there exactly once, each with a fee. `0008` was written by an earlier
version of this script that had no such check, and it shipped with 75 positions missing.

The correction is conservative, because the vet may have edited the catalogue since:

- a row whose text belongs to another position (its number was read from the description)
  is renumbered to the position it describes;
- a fee or a name is only changed where it still has the value read here, so an edit survives;
- the 291 names that differ editorially between the 2022 print and the consolidated text
  ("u." for "und", "Pelztiere" for "Pelztier") are left alone — only extraction damage is
  repaired;
- `hidden` is only switched where it still has the value read here, and a surgical position
  that a treatment line or a template already uses is never hidden.

Whatever it writes is **reviewed by hand** before it is committed.

Requires `pdftotext` (poppler-utils).
"""

from __future__ import annotations

import argparse
import csv
import re
import subprocess
import sys
from dataclasses import dataclass, field
from decimal import Decimal
from pathlib import Path

# A fee like "11,26" or "1 350,00".
FEE = r"\d{1,3}(?: \d{3})*,\d{2}"
# A position: its number at the left margin, the text, and — unless the text wraps — the fee.
ROW = re.compile(rf"^\s{{0,2}}(?P<number>\d{{1,4}})\s{{2,}}(?P<text>\S.*?)(?:\s{{2,}}(?P<fee>{FEE}))?\s*$")
# Any other line: indented text, possibly ending in the fee of a wrapped position.
LINE = re.compile(rf"^(?P<indent>\s*)(?P<text>\S.*?)(?:\s{{2,}}(?P<fee>{FEE}))?\s*$")
# Page furniture and column heads, repeated on every page.
NOISE = re.compile(
    r"^\s*(- Seite \d+ von \d+ -|Ein Service des Bundesministeriums|Justiz ‒ www\.gesetze"
    r"|Euro\s*$|Nr\.\s|Gebühr\s*$|Leistung\s*$)"
)
# Headings: a chapter ("4. Gastroenterologie, …"), a section ("4c) Chirurgische Behandlungen").
CHAPTER = re.compile(r"^(\d+\.|Teil [ABC])(\s|$)")
SECTION = re.compile(r"^\d*[a-z]\)\s")
# Operative headings — but "Nicht chirurgische Behandlungen" is the opposite.
SURGICAL_HEADING = re.compile(r"chirurg|operation|amputation|operativ", re.IGNORECASE)
NON_SURGICAL_HEADING = re.compile(r"nicht\s+chirurg", re.IGNORECASE)
# Positions whose own description is operative, wherever they stand.
SURGICAL_TEXT = re.compile(
    r"\b(Operation|operativ|Amputation|Resektion|Laparotomie|Thorakotomie|Osteosynthese"
    r"|Naht(?!untersuchung)|Kastration|Ovariektomie|Ovariohysterektomie|Enukleation"
    r"|Exstirpation|Trepanation|Arthrodese|Zystotomie|Enterotomie|Gastrotomie"
    r"|Sectio caesarea|Kaiserschnitt)\b",
    re.IGNORECASE,
)
# Words that follow a suspended hyphen ("Zucht- und Mastkaninchen").
CONJUNCTION = r"(?:und|oder|u\.|bzw\.|sowie)"

VAT_PERCENT = "19.000"
# Below this, a row's text and the schedule's text at its number describe different things.
UNRELATED = 0.35
# At or above this, and with an identical fee, a row's text is the schedule's text at that number.
SAME = 0.4


@dataclass
class Position:
    number: int
    lines: list[str]
    fee: Decimal | None
    heading: str
    column: int

    @property
    def name(self) -> str:
        return join_lines(self.lines)

    @property
    def hidden(self) -> bool:
        """Surgical positions are hidden: the practice offers no surgery (logic.md)."""
        operative_heading = bool(SURGICAL_HEADING.search(self.heading)) and not NON_SURGICAL_HEADING.search(
            self.heading
        )
        return operative_heading or bool(SURGICAL_TEXT.search(self.name))


@dataclass
class Row:
    number: str
    name: str
    net_price: Decimal
    hidden: bool


@dataclass
class Correction:
    renumbered: list[tuple[Row, Position]] = field(default_factory=list)
    fees: list[tuple[Row, Position]] = field(default_factory=list)
    names: list[tuple[Row, str]] = field(default_factory=list)
    inserted: list[Position] = field(default_factory=list)
    unhide: list[int] = field(default_factory=list)
    hide: list[int] = field(default_factory=list)
    # Same number and fee, very different text: left alone, listed for the review.
    dissimilar: list[tuple[Row, Position]] = field(default_factory=list)


def pdf_to_text(pdf: Path) -> list[str]:
    result = subprocess.run(["pdftotext", "-layout", str(pdf), "-"], capture_output=True, check=True, text=True)
    return result.stdout.splitlines()


def parse(lines: list[str]) -> dict[int, Position]:
    """Walks the schedule: numbered rows, their wrapped text, and the headings above them."""
    start = next(
        (index for index, line in enumerate(lines) if re.match(r"^\s{0,2}1\s{2,}Beratung", line)),
        None,
    )
    if start is None:
        raise SystemExit("position 1 not found — is this the consolidated GOT from gesetze-im-internet.de?")

    positions: dict[int, Position] = {}
    current: Position | None = None
    chapter = section = ""

    for line in lines[start:]:
        if not line.strip() or NOISE.match(line):
            continue

        row = ROW.match(line)
        # A number only starts a position if it is the next one; otherwise it is text
        # ("101 bis zu 150 Tieren" is a herd size, not position 101).
        if row and int(row.group("number")) == (current.number + 1 if current else 1):
            current = Position(
                number=int(row.group("number")),
                lines=[row.group("text")],
                fee=to_decimal(row.group("fee")),
                heading=section or chapter,
                column=line.index(row.group("text")),
            )
            positions[current.number] = current
            continue

        other = LINE.match(line)
        if other is None:
            continue
        # Text in the description column of a position still waiting for its fee wraps it;
        # anything else (centred, or after the fee) is a heading.
        if current and current.fee is None and len(other.group("indent")) <= current.column + 4:
            current.lines.append(other.group("text"))
            current.fee = to_decimal(other.group("fee"))
            continue

        heading = re.sub(r"\s{2,}", " ", line.strip())
        if CHAPTER.match(heading):
            chapter, section = heading, ""
        elif SECTION.match(heading):
            section = heading

    return positions


def validate(positions: dict[int, Position]) -> list[str]:
    """Everything that would make the catalogue incomplete or wrong."""
    problems = []
    last = max(positions, default=0)
    missing = [number for number in range(1, last + 1) if number not in positions]
    if missing:
        problems.append(f"numbers missing from 1–{last}: {missing}")
    without_fee = [number for number, position in positions.items() if position.fee is None]
    if without_fee:
        problems.append(f"positions without a fee: {without_fee}")
    return problems


def join_lines(lines: list[str]) -> str:
    """Joins wrapped text, deciding at each line break what a trailing hyphen meant."""
    text = ""
    for line in lines:
        line = re.sub(r"\s{2,}", " ", line.strip())
        if text.endswith("-"):
            if re.match(rf"^({CONJUNCTION}\b|,)", line):
                text += " " + line  # "Zucht-" / "und Mastkaninchen": a suspended hyphen
            elif line[:1].islower():
                text = text[:-1] + line  # "Unter-" / "suchung": hyphenation
            else:
                text += line  # "Nasen-" / "Rachen": a compound
        else:
            text = f"{text} {line}" if text else line
    # The 2022 print writes thousands with a dot ("1.001 bis 2.000 Tiere"), the consolidated
    # text with a space; keep the catalogue's spelling.
    text = re.sub(r"\b(\d{1,3})((?: \d{3})+)\b", lambda match: match.group(0).replace(" ", "."), text)
    return re.sub(r"\s+([,.;:])", r"\1", text).strip(" .")


def to_decimal(fee: str | None) -> Decimal | None:
    return Decimal(fee.replace(" ", "").replace(",", ".")) if fee else None


def words(text: str) -> set[str]:
    text = text.lower().replace("u.", "und")
    text = re.sub(r"(\d)[. ](\d{3})", r"\1\2", text)
    # Inflection differs between the two texts ("Tiere"/"Tieren", "Pelztiere"/"Pelztier").
    return {word.rstrip("n").rstrip("e") for word in re.findall(r"[a-zäöüß0-9]+", text) if len(word) > 1}


def similarity(left: str, right: str) -> float:
    a, b = words(left), words(right)
    return len(a & b) / len(a | b) if a | b else 1.0


def read_catalogue(path: Path) -> list[Row]:
    with path.open(encoding="utf-8", newline="") as handle:
        return [
            Row(
                number=record["got_number"],
                name=record["name"],
                net_price=Decimal(record["net_price"]),
                hidden=record["hidden"] in {"t", "true", "True"},
            )
            for record in csv.DictReader(handle)
        ]


def correct(positions: dict[int, Position], rows: list[Row]) -> Correction:
    """What turns the catalogue in `rows` into the schedule in `positions`."""
    numbers = [row.number for row in rows]
    duplicates = sorted({number for number in numbers if numbers.count(number) > 1})
    if duplicates:
        raise SystemExit(f"the catalogue holds GOT numbers twice, resolve by hand first: {duplicates}")

    correction = Correction()
    by_number: dict[int, Row] = {}

    # A row describing another position — its number is not in the schedule, or both its fee
    # and its text disagree with the position there — moves to the one its text and fee match.
    # Text alone is not enough: the 2022 print kept explanatory notes inside some names that
    # the consolidated text sets apart, so a correct row can read quite differently.
    misplaced = [
        row
        for row in rows
        if not row.number.isdigit()
        or int(row.number) not in positions
        or (
            positions[int(row.number)].fee != row.net_price
            and similarity(row.name, positions[int(row.number)].name) < UNRELATED
        )
    ]
    taken = {int(row.number) for row in rows if row not in misplaced}
    for row in misplaced:
        candidates = sorted(
            (
                (similarity(row.name, position.name), position)
                for position in positions.values()
                if position.number not in taken and position.fee == row.net_price
            ),
            key=lambda pair: pair[0],
            reverse=True,
        )
        if not candidates or candidates[0][0] < SAME:
            raise SystemExit(f"cannot place catalogue row {row.number} '{row.name}' — resolve by hand")
        position = candidates[0][1]
        taken.add(position.number)
        correction.renumbered.append((row, position))
        by_number[position.number] = row

    for row in rows:
        if row in misplaced:
            continue
        number = int(row.number)
        by_number[number] = row
        position = positions[number]
        if position.fee != row.net_price:
            correction.fees.append((row, position))
        if similarity(row.name, position.name) < UNRELATED:
            correction.dissimilar.append((row, position))
        repaired = repair_hyphens(row.name, positions.values())
        if repaired != row.name:
            correction.names.append((row, repaired))

    correction.inserted = [position for number, position in sorted(positions.items()) if number not in by_number]

    for number, row in sorted(by_number.items()):
        should_hide = positions[number].hidden
        if row.hidden and not should_hide:
            correction.unhide.append(number)
        elif should_hide and not row.hidden:
            correction.hide.append(number)
    return correction


def repair_hyphens(name: str, positions) -> str:
    """Restores suspended hyphens the first extraction swallowed ("Zuchtund" → "Zucht- und")."""
    stems = {
        stem
        for position in positions
        for stem in re.findall(rf"(\w+)- {CONJUNCTION}(?=\W|$)", position.name)
    }
    for stem in stems:
        name = re.sub(rf"\b{re.escape(stem)}({CONJUNCTION})(?=\W|$)", rf"{stem}- \1", name)
    return name


def quote(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def number_list(numbers: list[int]) -> str:
    return ", ".join(quote(str(number)) for number in numbers)


def to_sql(correction: Correction, source: Path) -> str:
    out = [
        "-- Corrections to the GOT 2022 catalogue imported by `0008_got_import.sql`.",
        "--",
        f"-- Generated by scripts/extract-got.py from {source.name} (the consolidated GOT,",
        "-- gesetze-im-internet.de) against the catalogue as 0008 left it, and reviewed by hand.",
        "-- Every change is guarded by the value it replaces, so an edit made through the",
        "-- application since survives. Treatment lines keep their own copy of number, name and",
        "-- fee, so nothing already billed changes.",
    ]

    if correction.renumbered:
        out += ["", "-- Rows whose number had been read from their own description (\"101 bis zu 150",
                "-- Tieren\"): they move to the position they describe, with that position's wording."]
        for row, position in correction.renumbered:
            out.append(
                f"UPDATE service SET got_number = {quote(str(position.number))}, name = {quote(position.name)}"
                f"\n WHERE type = 'got' AND got_number = {quote(row.number)} AND name = {quote(row.name)};"
            )

    if correction.fees:
        out += ["", "-- Fees read from the wrong place."]
        for row, position in correction.fees:
            out.append(
                f"UPDATE service SET net_price = {position.fee}"
                f"\n WHERE type = 'got' AND got_number = {quote(row.number)} AND net_price = {row.net_price};"
            )

    if correction.names:
        out += ["", "-- Suspended hyphens the extraction swallowed (\"Zuchtund\" → \"Zucht- und\")."]
        for row, repaired in correction.names:
            out.append(
                f"UPDATE service SET name = {quote(repaired)}"
                f"\n WHERE type = 'got' AND got_number = {quote(row.number)} AND name = {quote(row.name)};"
            )

    if correction.inserted:
        out += [
            "",
            f"-- The {len(correction.inserted)} positions the import missed: mostly fees of 333,34 € and",
            "-- more, whose three-fold rate the old parser could not read, and the positions whose",
            "-- numbers the misplaced rows above had taken.",
            "INSERT INTO service (type, name, got_number, factor, vat_percent, net_price, hidden, draft)",
            "VALUES",
            ",\n".join(
                f"    ('got', {quote(position.name)}, {quote(str(position.number))}, 100.000, {VAT_PERCENT},"
                f" {position.fee}, {'true' if position.hidden else 'false'}, false)"
                for position in correction.inserted
            )
            + ";",
        ]

    if correction.unhide:
        out += [
            "",
            "-- Non-surgical positions the import hid: its chapter test matched \"Nicht chirurgische",
            "-- Behandlungen\". Only where they are still hidden.",
            f"UPDATE service SET hidden = false\n WHERE type = 'got' AND hidden\n"
            f"   AND got_number IN ({number_list(correction.unhide)});",
        ]

    if correction.hide:
        out += [
            "",
            "-- Surgical positions the import left visible. Only where they are still visible and",
            "-- nothing uses them yet: a position already on a treatment line or in a template stays.",
            f"UPDATE service SET hidden = true\n WHERE type = 'got' AND NOT hidden\n"
            f"   AND got_number IN ({number_list(correction.hide)})\n"
            "   AND NOT EXISTS (SELECT 1 FROM treatment_item WHERE treatment_item.service_id = service.id)\n"
            "   AND NOT EXISTS (SELECT 1 FROM treatment_template_item"
            " WHERE treatment_template_item.service_id = service.id);",
        ]

    out.append("")
    return "\n".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pdf", type=Path, default=Path("requirements/GOT_2022.pdf"))
    parser.add_argument("--against", type=Path, help="CSV of the current catalogue (see above)")
    parser.add_argument("--out", type=Path, help="where to write the correction migration")
    arguments = parser.parse_args()

    positions = parse(pdf_to_text(arguments.pdf))
    problems = validate(positions)
    if problems:
        for problem in problems:
            print(f"✗ {problem}", file=sys.stderr)
        return 1
    hidden = sum(1 for position in positions.values() if position.hidden)
    print(f"✓ {len(positions)} positions, 1–{max(positions)}, each with a fee ({hidden} surgical, hidden)")

    if not arguments.against:
        return 0
    correction = correct(positions, read_catalogue(arguments.against))
    print(f"  renumbered {len(correction.renumbered)}, fees {len(correction.fees)}, names {len(correction.names)},")
    print(f"  inserted {len(correction.inserted)}, unhidden {len(correction.unhide)}, hidden {len(correction.hide)}")
    for row, position in correction.renumbered:
        print(f"    {row.number:>5} → {position.number:<5} {position.name[:70]}")
    if correction.dissimilar:
        print(f"  {len(correction.dissimilar)} rows keep number and fee but read differently — check by eye:")
        for row, position in correction.dissimilar:
            print(f"    {row.number:>5}  catalogue: {row.name[:60]}")
            print(f"    {'':>5}  schedule:  {position.name[:60]}")
    if arguments.out:
        arguments.out.write_text(to_sql(correction, arguments.pdf), encoding="utf-8")
        print(f"  written to {arguments.out} — review it before committing")
    return 0


if __name__ == "__main__":
    sys.exit(main())
