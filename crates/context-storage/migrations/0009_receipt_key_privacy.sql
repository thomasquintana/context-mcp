-- Caller-chosen keys may contain human content; retain only opaque intent handles.
CREATE TEMP TABLE hashed_receipts ON COMMIT DROP AS
SELECT principal,operation,encode(sha256(convert_to(idempotency_key,'UTF8')),'hex') AS idempotency_key,request_digest,receipt
FROM context.receipts;
DELETE FROM context.receipts;
INSERT INTO context.receipts SELECT * FROM hashed_receipts;
