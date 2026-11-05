//! What a barcode scanner typed: the code that identifies a pack, and — from a Data Matrix — the
//! batch and expiry of this one.
//!
//! A scanner set up as a keyboard types what it reads and ends with Enter. The pack's product
//! code comes as an EAN-13 (a GTIN), as a PZN barcode (Code 39, `-12345678`), or inside a Data
//! Matrix in one of the two formats the securPharm coding rules allow:
//!
//! - **IFA** (ISO/IEC 15434 format 06, ASC MH10 data identifiers):
//!   `[)>␞06␝9N<PPN>␝1T<batch>␝D<YYMMDD>␝S<serial>␞␄`, the PPN being `11` + PZN + two check
//!   characters;
//! - **GS1** (FNC1 + application identifiers): `01<GTIN/NTIN>17<YYMMDD>10<batch>␝21<serial>`,
//!   the NTIN being `4150` + PZN + check digit.
//!
//! Every product code is brought to one form, the 14-digit GTIN; a PZN becomes its NTIN. So the
//! EAN, the PZN barcode and either Data Matrix of one pack meet in the same key. The serial is
//! read past and dropped: it names one pack, and nothing in k-vet tracks single packs.

use chrono::{Datelike, NaiveDate};

/// Group separator: ends a variable-length field (FNC1 in a GS1 code, GS in an IFA one).
pub const GS: char = '\u{1d}';
/// Record separator and end of transmission: the envelope of the IFA format.
const RS: char = '\u{1e}';
const EOT: char = '\u{04}';

/// What one scan held. A plain EAN holds only the product; a Data Matrix usually all three.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// The product, as a 14-digit GTIN.
    pub gtin: Option<String>,
    pub batch: Option<String>,
    pub expiry: Option<NaiveDate>,
}

/// One product code — scanned or typed — as a 14-digit GTIN, or `None` when it is no valid code.
///
/// Takes an EAN-8, UPC-A, EAN-13 or GTIN-14 (check digit verified), a PZN (8 digits, or the old
/// 7, with or without the Code 39 `-` or a written `PZN`), and a PPN. Eight digits that pass the
/// PZN check are a PZN: in a practice's cupboard a PZN is far likelier than an EAN-8.
pub fn normalize(code: &str) -> Option<String> {
    let code = strip_symbology(code.trim());
    let code = code
        .strip_prefix("PZN")
        .or_else(|| code.strip_prefix("pzn"))
        .unwrap_or(code);
    let digits: String = code
        .chars()
        .filter(|character| !matches!(character, ' ' | '-'))
        .collect();
    if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    match digits.len() {
        7 => pzn_to_ntin(&format!("0{digits}")),
        8 => pzn_to_ntin(&digits).or_else(|| gs1_valid(&digits).then(|| pad14(&digits))),
        12 => {
            if gs1_valid(&digits) {
                Some(pad14(&digits))
            } else {
                ppn_to_ntin(&digits)
            }
        }
        13 | 14 => gs1_valid(&digits).then(|| pad14(&digits)),
        _ => None,
    }
}

/// How a code is shown: "PZN 12345678" for a PZN's NTIN, else the EAN-13 (or the GTIN-14).
pub fn display(gtin: &str) -> String {
    if let Some(pzn) = pzn_of(gtin) {
        return format!("PZN {pzn}");
    }
    match gtin.strip_prefix('0') {
        Some(ean) if ean.len() == 13 => ean.to_owned(),
        _ => gtin.to_owned(),
    }
}

/// The PZN inside an NTIN (`04150` + PZN + check digit), if the GTIN is one.
pub fn pzn_of(gtin: &str) -> Option<&str> {
    gtin.strip_prefix("04150")
        .and_then(|rest| rest.get(..8))
        .filter(|_| gtin.len() == 14)
}

/// Reads one scan: an IFA or GS1 Data Matrix (or GS1-128), or a single linear code.
///
/// What cannot be read is left out rather than guessed: the vet sees every value in an editable
/// field before anything is booked.
pub fn read(raw: &str) -> Scan {
    let text = strip_symbology(raw.trim_matches(|character: char| {
        character.is_whitespace() || character == '\r' || character == '\n'
    }));
    if let Some(body) = text.strip_prefix("[)>") {
        return read_ifa(body);
    }
    if text.starts_with('(') {
        return read_gs1_hri(text);
    }
    if text.starts_with("01") && text.len() > 16 {
        return read_gs1(text);
    }
    Scan {
        gtin: normalize(text),
        ..Scan::default()
    }
}

/// The AIM symbology identifier some scanners put in front (`]d2` Data Matrix, `]C1` GS1-128,
/// `]E0` EAN-13, `]A0` Code 39, …).
fn strip_symbology(text: &str) -> &str {
    match text.strip_prefix(']') {
        Some(rest) if rest.len() >= 2 => rest.get(2..).unwrap_or(""),
        _ => text,
    }
}

/// IFA: `06␝` then data elements separated by GS, each a data identifier and its value.
fn read_ifa(body: &str) -> Scan {
    let mut scan = Scan::default();
    let elements = body
        .split([GS, RS, EOT])
        .map(str::trim)
        .filter(|element| !element.is_empty() && *element != "06");
    for element in elements {
        let (identifier, value) = split_data_identifier(element);
        match identifier {
            "9N" => scan.gtin = ppn_to_ntin(value),
            "8P" => scan.gtin = normalize(value),
            "1T" => scan.batch = non_empty(value),
            "D" => scan.expiry = expiry(value),
            _ => {}
        }
    }
    scan
}

/// An ASC MH10 data identifier is up to three digits and one capital letter: `9N`, `1T`, `D`,
/// `S`, `16D`, `33L`.
fn split_data_identifier(element: &str) -> (&str, &str) {
    let digits = element
        .chars()
        .take_while(char::is_ascii_digit)
        .count()
        .min(3);
    match element.chars().nth(digits) {
        Some(letter) if letter.is_ascii_uppercase() => {
            let end = digits + 1;
            (
                element.get(..end).unwrap_or(""),
                element.get(end..).unwrap_or(""),
            )
        }
        _ => ("", element),
    }
}

/// How long an application identifier's value is: `Some(n)` fixed, `None` up to the next GS.
/// `Err` for an AI not known here, after which nothing more can be read reliably.
fn ai_length(ai: &str) -> Result<Option<usize>, ()> {
    match ai {
        "00" => Ok(Some(18)),
        "01" | "02" => Ok(Some(14)),
        "11" | "12" | "13" | "15" | "16" | "17" => Ok(Some(6)),
        "20" => Ok(Some(2)),
        "10" | "21" | "22" | "30" | "37" | "240" | "241" | "250" | "251" | "710" | "711"
        | "712" | "713" | "714" | "715" | "8200" => Ok(None),
        _ if ai.len() == 2 && ai.starts_with('9') => Ok(None),
        _ if ai.len() == 3 && ("410"..="417").contains(&ai) => Ok(Some(13)),
        _ if ai.len() == 4 && ("3100"..="3699").contains(&ai) => Ok(Some(6)),
        _ => Err(()),
    }
}

/// A GS1 element string as the scanner sends it: AIs and values back to back, variable-length
/// values ended by GS. A variable value without GS runs to the end — GS1 puts those last.
fn read_gs1(text: &str) -> Scan {
    let mut scan = Scan::default();
    let mut rest = text;
    while !rest.is_empty() {
        rest = rest.trim_start_matches(GS);
        let Some((ai, length)) = [2, 3, 4].into_iter().find_map(|size| {
            let ai = rest.get(..size)?;
            ai_length(ai).ok().map(|length| (ai, length))
        }) else {
            break;
        };
        let after = rest.get(ai.len()..).unwrap_or("");
        let (value, next) = match length {
            Some(length) => (after.get(..length).unwrap_or(after), after.get(length..)),
            None => match after.split_once(GS) {
                Some((value, next)) => (value, Some(next)),
                None => (after, None),
            },
        };
        apply_ai(&mut scan, ai, value);
        rest = next.unwrap_or("");
    }
    scan
}

/// The same written out for people: `(01)04150123456782(17)290831(10)ABC`. Some scanners can
/// be set to send this, which needs no GS at all.
fn read_gs1_hri(text: &str) -> Scan {
    let mut scan = Scan::default();
    for part in text.split('(').filter(|part| !part.is_empty()) {
        if let Some((ai, value)) = part.split_once(')') {
            apply_ai(&mut scan, ai, value.trim_end_matches(GS));
        }
    }
    scan
}

fn apply_ai(scan: &mut Scan, ai: &str, value: &str) {
    match ai {
        "01" | "02" => scan.gtin = normalize(value),
        // A German reimbursement number (NHRN) is the PZN.
        "710" if scan.gtin.is_none() => scan.gtin = normalize(value),
        "10" => scan.batch = non_empty(value),
        "17" => scan.expiry = expiry(value),
        _ => {}
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// `YYMMDD`. Day `00` means the month as a whole — its last day — as packs print "08.2029".
fn expiry(value: &str) -> Option<NaiveDate> {
    if value.len() != 6 || !value.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let number = |range: std::ops::Range<usize>| value.get(range)?.parse::<u32>().ok();
    let year = 2000 + i32::try_from(number(0..2)?).ok()?;
    let month = number(2..4)?;
    match number(4..6)? {
        0 => last_of_month(year, month),
        day => NaiveDate::from_ymd_opt(year, month, day),
    }
}

fn last_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    next.pred_opt().filter(|last| last.month() == first.month())
}

fn digit_values(digits: &str) -> Vec<u32> {
    digits
        .chars()
        .filter_map(|digit| digit.to_digit(10))
        .collect()
}

/// The GS1 mod-10 check: weights 3 and 1 from the right, the check digit last.
fn gs1_valid(digits: &str) -> bool {
    let values = digit_values(digits);
    let Some((check, body)) = values.split_last() else {
        return false;
    };
    gs1_check_digit(body) == *check
}

fn gs1_check_digit(body: &[u32]) -> u32 {
    let sum: u32 = body
        .iter()
        .rev()
        .enumerate()
        .map(|(position, digit)| digit * if position % 2 == 0 { 3 } else { 1 })
        .sum();
    (10 - sum % 10) % 10
}

fn pad14(digits: &str) -> String {
    format!("{digits:0>14}")
}

/// An 8-digit PZN's check: the first seven digits weighted 1 to 7, mod 11 — a remainder of 10
/// is never issued.
fn pzn_valid(pzn: &str) -> bool {
    let values = digit_values(pzn);
    if values.len() != 8 {
        return false;
    }
    let sum: u32 = values
        .iter()
        .take(7)
        .zip(1..)
        .map(|(digit, weight)| digit * weight)
        .sum();
    let check = sum % 11;
    check != 10 && values.last() == Some(&check)
}

/// A valid PZN's NTIN as GTIN-14: `0` + `4150` + PZN + GS1 check digit.
fn pzn_to_ntin(pzn: &str) -> Option<String> {
    if !pzn_valid(pzn) {
        return None;
    }
    let body = format!("4150{pzn}");
    let check = gs1_check_digit(&digit_values(&body));
    Some(format!("0{body}{check}"))
}

/// A PPN (`11` + PZN + two check characters) to the NTIN of its PZN. The check characters are
/// the sum of the characters' ASCII values weighted from 2 upwards, mod 97 — the securPharm
/// coding rules' example: PZN 12345678 gives `111234567842`.
fn ppn_to_ntin(ppn: &str) -> Option<String> {
    let ppn = ppn.trim();
    let body = ppn.get(..ppn.len().checked_sub(2)?)?;
    let check: u32 = ppn.get(body.len()..)?.parse().ok()?;
    let sum: u32 = body
        .bytes()
        .zip(2..)
        .map(|(byte, weight)| u32::from(byte) * weight)
        .sum();
    if sum % 97 != check {
        return None;
    }
    pzn_to_ntin(body.strip_prefix("11")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NTIN: &str = "04150123456782"; // PZN 12345678, as in the securPharm coding rules

    fn date(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(year, month, day)
    }

    #[test]
    fn every_product_code_of_a_pack_meets_in_one_gtin() {
        // EAN-13, as most packs carry it, and the same as GTIN-14.
        assert_eq!(
            normalize("4006381333931").as_deref(),
            Some("04006381333931")
        );
        assert_eq!(
            normalize("04006381333931").as_deref(),
            Some("04006381333931")
        );
        // The PZN, however it comes: typed, from its Code 39, written out, the old 7 digits.
        for pzn in ["12345678", "-12345678", "PZN 12345678", "PZN-12345678"] {
            assert_eq!(normalize(pzn).as_deref(), Some(NTIN), "{pzn}");
        }
        assert_eq!(normalize("1234562").as_deref(), Some("04150012345623"));
        // A PPN, which only an IFA Data Matrix carries.
        assert_eq!(normalize("111234567842").as_deref(), Some(NTIN));
        // An AIM prefix is not part of the code.
        assert_eq!(
            normalize("]E04006381333931").as_deref(),
            Some("04006381333931")
        );
    }

    #[test]
    fn a_wrong_check_digit_is_no_code() {
        assert_eq!(normalize("4006381333932"), None);
        assert_eq!(normalize("12345679"), None, "neither PZN nor EAN-8");
        assert_eq!(normalize("111234567843"), None, "PPN check off by one");
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("Metacam"), None);
        // Eight digits failing the PZN check but passing the EAN-8 one are an EAN-8.
        assert_eq!(normalize("12345670").as_deref(), Some("00000012345670"));
    }

    #[test]
    fn a_code_is_shown_as_people_know_it() {
        assert_eq!(display(NTIN), "PZN 12345678");
        assert_eq!(display("04006381333931"), "4006381333931");
        assert_eq!(display("14006381333938"), "14006381333938");
        assert_eq!(pzn_of(NTIN), Some("12345678"));
        assert_eq!(pzn_of("04006381333931"), None);
    }

    #[test]
    fn an_ifa_data_matrix_gives_product_batch_and_expiry() {
        let raw = format!(
            "[)>{RS}06{GS}9N111234567842{GS}1T12345ABCD{GS}D290800{GS}S12345ABCDEF98765{RS}{EOT}"
        );
        assert_eq!(
            read(&raw),
            Scan {
                gtin: Some(NTIN.to_owned()),
                batch: Some("12345ABCD".to_owned()),
                // Day 00: the month, i.e. its last day.
                expiry: date(2029, 8, 31),
            }
        );
        // The order of the elements is free.
        let reordered = format!("[)>{RS}06{GS}D290815{GS}1TX-7{GS}9N111234567842{RS}{EOT}");
        assert_eq!(read(&reordered).expiry, date(2029, 8, 15));
        assert_eq!(read(&reordered).batch.as_deref(), Some("X-7"));
    }

    #[test]
    fn a_gs1_data_matrix_gives_product_batch_and_expiry() {
        // AI 01 the NTIN, 17 the expiry, 10 the batch (ended by GS), 21 the serial.
        let raw = format!("]d201041501234567821729083110ABC123{GS}21SERIAL0001");
        assert_eq!(
            read(&raw),
            Scan {
                gtin: Some(NTIN.to_owned()),
                batch: Some("ABC123".to_owned()),
                expiry: date(2029, 8, 31),
            }
        );
        // Variable fields last and no GS at all: the batch runs to the end.
        assert_eq!(
            read("01040063813339311728022910K77").batch.as_deref(),
            Some("K77")
        );
        assert_eq!(
            read("01040063813339311728022910K77").expiry,
            date(2028, 2, 29)
        );
        // Written out for people, as some scanners can send it.
        let hri = read("(01)04150123456782(17)290800(10)AB 12(21)XYZ");
        assert_eq!(hri.gtin.as_deref(), Some(NTIN));
        assert_eq!(hri.batch.as_deref(), Some("AB 12"));
        assert_eq!(hri.expiry, date(2029, 8, 31));
    }

    #[test]
    fn a_linear_code_gives_the_product_only() {
        assert_eq!(
            read("4006381333931\r\n"),
            Scan {
                gtin: Some("04006381333931".to_owned()),
                ..Scan::default()
            }
        );
        assert_eq!(read("-12345678").gtin.as_deref(), Some(NTIN));
    }

    #[test]
    fn what_is_no_code_gives_nothing() {
        assert_eq!(read("hallo"), Scan::default());
        assert_eq!(read(""), Scan::default());
        // An impossible date is left out, the rest still read.
        let scan = read(&format!("0104150123456782171313130{GS}10A"));
        assert_eq!(scan.gtin.as_deref(), Some(NTIN));
        assert_eq!(scan.expiry, None);
    }
}
