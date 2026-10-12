import json
import unittest
from audit_autoplay import audit_lines


def row(event_name, **extra):
    value = dict(schema_version=1, ts_ms=1, operation_id=1, attempt=1, retry=0,
                 action="ron", actor=0, target=1, flow_id=1, window_step=10,
                 event=event_name, phase="succeeded", reason=None, msg_id=None,
                 operation_type=None, error_code=None)
    if event_name == "client_request_matched":
        value.update(msg_id=42, operation_type=9)
    value.update(extra)
    return json.dumps(value)


class AuditTests(unittest.TestCase):
    def test_requires_all_same_attempt_evidence_and_accepts_fast_feedback_order(self):
        complete = [row(event) for event in ("client_request_matched", "server_feedback_matched",
                    "press_accepted", "release_accepted", "operation_verified")]
        self.assertEqual(audit_lines(complete)["verified_ron"], 1)
        for omit in range(len(complete)):
            self.assertEqual(audit_lines(complete[:omit] + complete[omit+1:])["verified_ron"], 0)
        for field, value in [("attempt", 2), ("window_step", 11), ("flow_id", 2), ("target", 2)]:
            mixed = complete[:-1] + [row("operation_verified", **{field: value})]
            self.assertEqual(audit_lines(mixed)["verified_ron"], 0)
        tsumo = [json.loads(line) for line in complete]
        for record in tsumo:
            record.update(action="tsumo", target=0)
            if record["event"] == "client_request_matched":
                record["operation_type"] = 8
        self.assertEqual(audit_lines(map(json.dumps, tsumo))["verified_tsumo"], 1)

    def test_rpc_ack_manual_conflict_and_empty_legacy_log_are_not_success(self):
        self.assertEqual(audit_lines([row("client_response_accepted"), row("operation_verified")])["verified_ron"], 0)
        complete = [row(event) for event in ("press_accepted", "release_accepted", "client_request_matched", "server_feedback_matched", "operation_verified")]
        self.assertEqual(audit_lines(complete + [row("client_request_rejected")])["verified_ron"], 0)
        self.assertEqual(audit_lines(complete + [row("press_rejected")])["verified_ron"], 0)
        self.assertEqual(audit_lines(complete + [row("release_rejected")])["verified_ron"], 0)
        self.assertEqual(audit_lines(["{}", "invalid"])["invalid_records"], 2)
        self.assertEqual(audit_lines([row("operation_verified", password="FAKE_SECRET")])["verified_ron"], 0)
        for field in ("action", "event", "phase", "reason"):
            self.assertEqual(audit_lines([row("operation_verified", **{field: "FAKE_SECRET"})])["invalid_records"], 1)


if __name__ == "__main__":
    unittest.main()
