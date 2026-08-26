-- A runtime token is minted for one dispatch and must die with it. Without a
-- binding there was nothing to revoke by: the supervisor discards the
-- plaintext after spawn, so credentials outlived their runs and could still
-- claim new leases (smoke walk 4, S2).
ALTER TABLE token ADD COLUMN run_id TEXT REFERENCES run(id);
