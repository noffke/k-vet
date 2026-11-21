/**
 * Pack codes for the specs. A code belongs to one drug, and a worker's specs share one database, so
 * every test makes its own: a fresh PZN with valid check digits, and the Data Matrix contents a
 * pack with it would carry (securPharm coding rules: PPN = "11" + PZN + two check characters,
 * NTIN = "4150" + PZN + GS1 check digit).
 */

const GS = '\u001d'
const RS = '\u001e'
const EOT = '\u0004'

/** A PZN no other test uses: seven digits from the clock and a counter, plus the mod-11 check. */
let counter = 0
export function uniquePzn(): string {
  for (;;) {
    counter += 1
    const body = `${(Date.now() + counter * 7919) % 10_000_000}`.padStart(7, '0')
    const check = [...body].reduce((sum, digit, i) => sum + Number(digit) * (i + 1), 0) % 11
    if (check !== 10) return `${body}${check}`
  }
}

/** The PPN of a PZN: ASCII values weighted from 2 upwards, mod 97. */
function ppnOf(pzn: string): string {
  const body = `11${pzn}`
  const sum = [...body].reduce(
    (total, character, i) => total + character.charCodeAt(0) * (i + 2),
    0,
  )
  return `${body}${String(sum % 97).padStart(2, '0')}`
}

/** An IFA Data Matrix: PPN, batch, expiry as YYMMDD (day 00 for the month as a whole). */
export function ifaMatrix(pzn: string, batch: string, expiry: string): string {
  return `[)>${RS}06${GS}9N${ppnOf(pzn)}${GS}1T${batch}${GS}D${expiry}${GS}S${pzn}SN${RS}${EOT}`
}
