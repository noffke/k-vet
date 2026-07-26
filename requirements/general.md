# General Project Guidelines
* UI must support i18n, en-US and de-DE. de-DE is default language for user-facing documentation. code and technical documentation uses en-US. Configuration file(s) are commented in both en-US and de-DE
* UI must be responsive, target laptop/desktop screens and mobile (Pixel 9a)
* For now, no multi tenancy, no multi user. Just a single user with login and session. User and password are supplied via configuration
* UI should auto-save all user-input (like google docs, sheets, etc.), no explicit save. Make absolutely sure this works, there's nothing more frustrating than loosing text you just entered.
* UI should be visually pleasing but UX and usability is king
* Every entity has their own sequence based primary key
* database timestamps are stored with timezone
* rust code:
  * no unwrap, expect
  * full semver in Cargo.toml
  * clippy, rustfmt for linting and formatting
* typescript
  * strict
  * biome for linting and formatting, and knip for checks
  * full semver in package.json
* all code changes need tests
* for the UI, use the color scheme from the practice's website