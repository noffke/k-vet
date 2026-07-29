/**
 * Credentials of the single user the E2E stack boots with.
 * The hash is an argon2id hash of `E2E_PASSWORD`, produced with
 * `k-vet-backend --hash-password`.
 */
export const E2E_USERNAME = 'tierarzt'
export const E2E_PASSWORD = 'test1234'
export const E2E_PASSWORD_HASH =
  '$argon2id$v=19$m=19456,t=2,p=1$sv7lVNC/98GKqHKTWF2RnA$e/w5YttGxthEXoRvReBLS4+Y1NQOvwaFcRGrtg6JXNU'
