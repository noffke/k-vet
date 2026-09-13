# Contributing

Short version: **pull requests are not accepted, and issues are welcome.**

k-vet is the practice-management system for one German veterinary practice, built for one vet on
one Raspberry Pi. It is published so the approach can be read, borrowed and forked — not because
it is looking for contributors. Requirements come from the practice itself, and the decisions in
`requirements/`, `specs/001-vet-practice-app/` and `.specify/memory/constitution.md` were made
against that one installation's needs, which is not a basis on which outside changes can be
fairly judged.

So pull requests from forks are closed automatically by
`.github/workflows/close-external-prs.yml`. That is not a verdict on the change — there is simply
no review process behind it to give you a fair hearing, and leaving a PR open for months would be
worse than saying so plainly.

## What is useful

- **Bug reports**, especially ones that show the app computing a wrong number. The money rules
  live in `src/domain/money.rs` (AMPreisV § 3/§ 4/§ 10 drug pricing, GOT § 10 Wegegeld, VAT per
  line) and are the part most worth a second pair of eyes.
- **Corrections to the domain reasoning.** If a German veterinary fee or tax rule is applied
  wrongly, saying so — with the statute — is more valuable than a patch.
- **Questions about how something works.** They tend to reveal where the documentation is thin.

Security problems go through `SECURITY.md`, privately, not through an issue.

## Forking

The licence is [GPL-3.0](LICENSE), so forking and taking this wherever you need is expressly
fine — that is the point of publishing it. Two things to know before you run it:

- The GOT 2022 fee catalogue PDF is deliberately not in the repository; the extracted fees are in
  migration `0008_got_import.sql`, and `scripts/extract-got.py` documents where the source came
  from. The fees are a federal regulation; the typeset PDF is not ours to redistribute.
- Practice identity, bank details and tax numbers are configuration, not code. Everything you
  will find in the tests and in `review.md` is placeholder data.

`README.md` covers the layout, and `CLAUDE.md` is the working guide to the codebase — the
contract pipeline, the draft-row and auto-save conventions, and the sqlx rules that will bite you
first.
