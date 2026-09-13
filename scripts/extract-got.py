#!/usr/bin/env python3
"""Extracts the GOT 2022 fee schedule into a SQL migration (T061, research R9).

The Gebührenverzeichnis of `requirements/GOT_2022.pdf` lists every billable position as

    Lfd.   Grundleistung                        1-fach   2-fach   3-fach
    Nr.                                           €        €        €

      1   Beratung im einzelnen Fall ohne
          Untersuchung                          11,26    22,52    33,78

so a position is a running number, a description spanning one or more lines, and three
rates. Only the single rate is stored: the factor lives on the treatment line (FR-021).

Table extraction from a PDF is never exact, which is why this script writes a migration
that is **reviewed by hand** before it is committed, and prints the spot checks the
reviewer should confirm.

The PDF itself is deliberately not committed — it is the Bundesanzeiger typesetting of the
GOT and not ours to redistribute. The extracted fees live in migration `0008_got_import.sql`,
which every database already has, so this script only needs to run again if the fee schedule
changes. Put a copy of the catalogue at `requirements/GOT_2022.pdf` (git ignores it) or pass
`--pdf`.

Usage:
    scripts/extract-got.py [--pdf requirements/GOT_2022.pdf] [--out k-vet-backend/migrations/0008_got_import.sql]

Requires `pdftotext` (poppler-utils).
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

# A price like "11,26" or "1.234,56".
PRICE = r"\d{1,3}(?:\.\d{3})*,\d{2}"
# A table row ends with the three rates; the description may share the line.
ROW = re.compile(rf"^\s*(?P<number>\d{{1,4}}[a-z]?)?\s+(?P<text>.*?)\s+(?P<single>{PRICE})\s+{PRICE}\s+{PRICE}\s*$")
# A continuation line: description only, no rates.
NUMBER_START = re.compile(r"^\s*(?P<number>\d{1,4}[a-z]?)\s+(?P<text>\S.*)$")
# Page furniture we never want inside a description.
NOISE = re.compile(r"^\s*(Lfd\.|Nr\.|\d+\s*$|Teil [ABC]\b|Gebührenverzeichnis|Seite\s*$)")
# Chapter headings that mark operative work (hidden by default per R9).
SURGICAL = re.compile(r"chirurg", re.IGNORECASE)
# Positions whose own description is operative even outside a surgical chapter.
SURGICAL_TEXT = re.compile(
    r"\b(Operation|operativ|Amputation|Resektion|Laparotomie|Thorakotomie|Osteosynthese"
    r"|Naht(?!untersuchung)|Kastration|Ovariektomie|Ovariohysterektomie|Enukleation"
    r"|Exstirpation|Trepanation|Arthrodese|Zystotomie|Enterotomie|Gastrotomie"
    r"|Sectio caesarea|Kaiserschnitt)\b",
    re.IGNORECASE,
)

VAT_PERCENT = "19.000"


@dataclass
class Position:
    number: str
    name: str
    single_rate: str
    surgical_chapter: str | None

    @property
    def hidden(self) -> bool:
        """Surgical positions are hidden: the practice offers no surgery (logic.md)."""
        return self.surgical_chapter is not None or bool(SURGICAL_TEXT.search(self.name))


def pdf_to_text(pdf: Path) -> list[str]:
    result = subprocess.run(
        ["pdftotext", "-layout", str(pdf), "-"],
        capture_output=True,
        check=True,
        text=True,
    )
    return result.stdout.splitlines()


def parse(lines: list[str]) -> list[Position]:
    """Walks the schedule, joining wrapped descriptions with their rates."""
    # Everything before the schedule itself is the ordinance's text.
    start = next(
        (index for index, line in enumerate(lines) if line.strip() == "Gebührenverzeichnis" and index > 700),
        0,
    )

    positions: list[Position] = []
    pending: list[str] = []
    pending_number: str | None = None
    chapter: str | None = None
    seen: set[str] = set()

    for line in lines[start:]:
        stripped = line.strip()

        # Chapter headings appear twice (running head plus body); either is enough.
        if stripped and not re.search(PRICE, stripped) and len(stripped) < 60:
            if SURGICAL.search(stripped):
                chapter = stripped
            elif re.match(r"^(Teil [ABC]|[A-ZÄÖÜ][\wäöüß ,.\-/]{2,})$", stripped) and not NOISE.match(line):
                # A non-surgical heading closes the surgical chapter.
                chapter = None

        row = ROW.match(line)
        if row:
            number = row.group("number") or pending_number
            text = " ".join(part for part in [*pending, row.group("text")] if part).strip()
            pending, pending_number = [], None
            if not number or not text:
                continue
            # Repeated running heads can duplicate a position; the first wins.
            if number in seen:
                continue
            seen.add(number)
            positions.append(
                Position(
                    number=number,
                    name=normalise(text),
                    single_rate=to_decimal(row.group("single")),
                    surgical_chapter=chapter,
                )
            )
            continue

        if NOISE.match(line) or not stripped:
            # A blank line ends an unfinished description (page break, heading, …).
            if not stripped:
                pending, pending_number = [], None
            continue

        start_of_position = NUMBER_START.match(line)
        if start_of_position and not pending:
            pending_number = start_of_position.group("number")
            pending = [start_of_position.group("text")]
        else:
            pending.append(stripped)

    return positions


def normalise(text: str) -> str:
    """Repairs the artefacts of column extraction: hyphenation and doubled spaces."""
    text = re.sub(r"(\w)-\s+(\w)", r"\1\2", text)
    text = re.sub(r"\s{2,}", " ", text)
    text = re.sub(r"\s+([,.;:])", r"\1", text)
    return text.strip(" .")


def to_decimal(price: str) -> str:
    return price.replace(".", "").replace(",", ".")


def to_sql(positions: list[Position], pdf: Path) -> str:
    lines = [
        "-- GOT 2022 fee schedule (T061, research R9).",
        "--",
        f"-- Generated by scripts/extract-got.py from {pdf.name} and reviewed by hand.",
        "-- Only the single rate is imported: the factor lives on the treatment line, and a",
        "-- line pins its own price copy anyway (FR-021, FR-028).",
        "--",
        "-- Surgical positions are imported with `hidden = true`; the practice offers no",
        "-- surgery, so they stay out of pickers and lists until that changes (logic.md).",
        "",
        "INSERT INTO service (type, name, got_number, factor, vat_percent, gross_price,",
        "                     hidden, draft)",
        "VALUES",
    ]
    values = [
        "    ('got', {name}, {number}, 100.000, {vat}, {price}, {hidden}, false)".format(
            name=quote(position.name),
            number=quote(position.number),
            vat=VAT_PERCENT,
            price=position.single_rate,
            hidden="true" if position.hidden else "false",
        )
        for position in positions
    ]
    lines.append(",\n".join(values) + ";")
    lines.append("")
    return "\n".join(lines)


def quote(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, default=Path("requirements/GOT_2022.pdf"))
    parser.add_argument(
        "--out", type=Path, default=Path("k-vet-backend/migrations/0008_got_import.sql")
    )
    arguments = parser.parse_args()

    positions = parse(pdf_to_text(arguments.pdf))
    if not positions:
        print("no positions found — did the PDF layout change?", file=sys.stderr)
        return 1

    arguments.out.write_text(to_sql(positions, arguments.pdf), encoding="utf-8")

    surgical = sum(1 for position in positions if position.hidden)
    print(f"{len(positions)} positions written to {arguments.out} ({surgical} hidden as surgical)")
    print("\nSpot checks for the review (compare against the PDF):")
    for position in positions[:3]:
        print(f"  {position.number:>5}  {position.single_rate:>8}  {position.name[:70]}")
    print("  …")
    for position in positions[-3:]:
        print(f"  {position.number:>5}  {position.single_rate:>8}  {position.name[:70]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
