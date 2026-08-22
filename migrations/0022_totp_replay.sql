-- Migration 0022: TOTP replay protection (review A-4, RFC 6238 §5.2)
--
-- last_used_timestep records the RFC 6238 counter of the most recently
-- accepted TOTP code. login_2fa records the matched timestep atomically
-- with the guard `(last_used_timestep IS NULL OR last_used_timestep < ?)`;
-- a replayed or older code makes the guarded UPDATE match zero rows and is
-- rejected. Timesteps are wall-clock derived, so they only ever increase.

ALTER TABLE totp_secrets ADD COLUMN last_used_timestep INTEGER;
