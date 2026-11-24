-- Preserve knowable legacy create IDs before transfer sessions are retired.
-- A receipt whose upload is already gone cannot be backfilled from this history.
UPDATE context.receipts r
SET receipt = r.receipt || jsonb_build_object('asset', u.asset, 'revision', u.revision)
FROM context.uploads u
WHERE r.operation = 'create_upload'
  AND r.principal = u.principal
  AND (r.receipt->>'upload')::uuid = u.id;
