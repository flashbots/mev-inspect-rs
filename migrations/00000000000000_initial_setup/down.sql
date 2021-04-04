DROP INDEX IF EXISTS internal_calls_callee_class_idx;
DROP INDEX IF EXISTS internal_calls_caller_class_idx;
DROP INDEX IF EXISTS internal_calls_callee_idx;
DROP INDEX IF EXISTS internal_calls_caller_idx;
DROP INDEX IF EXISTS internal_calls_protocol_idx;
DROP TABLE IF EXISTS internal_calls;

--DROP TABLE IF EXISTS mev_inspections;
DROP INDEX IF EXISTS event_logs_block_signature_idx;
DROP INDEX IF EXISTS event_logs_txs_idx;
DROP INDEX IF EXISTS event_logs_signature_idx;
DROP INDEX IF EXISTS event_logs_address_idx;
DROP TABLE IF EXISTS event_logs;

DROP TABLE IF EXISTS ignored_targets;
DROP TABLE IF EXISTS protocols;
DROP TABLE IF EXISTS known_bots;
DROP TABLE IF EXISTS addressbook;

DROP TYPE IF EXISTS call_classification;
DROP TYPE IF EXISTS call_type;
