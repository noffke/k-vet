/**
 * Credentials of the single user the E2E stack boots with.
 *
 * The hash is an argon2id hash of `E2E_PASSWORD` with the cheapest parameters (`m=8,t=1,p=1`),
 * the one the backend's own tests use. The server reads the parameters from the hash, so every
 * login — each `signIn`, each seed helper's API login, each DAV request — costs microseconds
 * instead of a production-strength argon2 run in a debug binary. Production hashes
 * (`k-vet-backend --hash-password`) are unchanged.
 */
export const E2E_USERNAME = 'tierarzt'
export const E2E_PASSWORD = 'test1234'
export const E2E_PASSWORD_HASH =
  '$argon2id$v=19$m=8,t=1,p=1$y/0sUMHarkKgaBpzUS9UGA$5hD61A/y1ge0S53p4cTVsgncVRm4QCAEuF/d5rToM04'
