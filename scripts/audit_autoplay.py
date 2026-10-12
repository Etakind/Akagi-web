#!/usr/bin/env python3
"""Count verified auto-wins from a closed-schema autoplay.log; never echo payloads."""
import argparse
import json
from collections import defaultdict
from pathlib import Path


def audit_lines(lines):
    facts = defaultdict(set)
    wins = set()
    declared = set()
    invalid = 0
    allowed = {"schema_version", "ts_ms", "operation_id", "attempt", "retry", "action",
               "actor", "target", "flow_id", "window_step", "event", "phase", "reason",
               "msg_id", "operation_type", "error_code"}
    actions = {"discard", "chi", "pon", "daiminkan", "ankan", "kakan", "riichi",
               "ron", "tsumo", "kita", "abortive_draw", "pass", "unknown"}
    events = {"operation_started", "state_changed", "press_attempt", "press_accepted",
              "press_rejected", "release_accepted", "release_rejected",
              "client_request_matched", "client_request_rejected", "client_response_accepted",
              "client_response_rejected", "server_feedback_matched", "server_feedback_conflict",
              "operation_verified"}
    phases = {"preparing", "scheduled", "executing", "awaiting_feedback", "succeeded",
              "cancelled", "failed", "unconfirmed"}
    reasons = {None, "autoplay_disabled", "window_expired", "disconnected", "page_invalid",
               "manual_takeover", "request_id_reused", "input_conflict", "feedback_conflict",
               "server_error", "server_confirmed", "feedback_timeout", "cdp_failed",
               "operation_aborted", "unknown"}
    for line in lines:
        try:
            row = json.loads(line)
            if not isinstance(row, dict) or set(row) != allowed or type(row["schema_version"]) is not int or row["schema_version"] != 1:
                raise ValueError()
            if type(row["ts_ms"]) is not int or type(row["retry"]) is not int or row["retry"] < 0:
                raise ValueError()
            if row["action"] not in actions or row["event"] not in events or row["phase"] not in phases or row["reason"] not in reasons:
                raise ValueError()
            for field, maximum in (("msg_id", 65535), ("operation_type", 11)):
                value = row[field]
                if value is not None and (type(value) is not int or not 0 <= value <= maximum):
                    raise ValueError()
            if row["error_code"] is not None and type(row["error_code"]) is not int:
                raise ValueError()
            if row["action"] not in ("ron", "tsumo"):
                continue
            for field in ("operation_id", "attempt", "flow_id", "window_step", "actor", "target"):
                if type(row[field]) is not int or row[field] < 0:
                    raise ValueError()
            if row["actor"] > 3 or row["target"] > 3:
                raise ValueError()
            if (row["action"] == "tsumo") != (row["actor"] == row["target"]):
                raise ValueError()
            key = tuple(row[field] for field in ("operation_id", "attempt", "action", "actor", "target", "flow_id", "window_step"))
            wins.add(row["operation_id"])
            event = row["event"]
            if event == "client_request_matched":
                if row["operation_type"] != (8 if row["action"] == "tsumo" else 9) or type(row["msg_id"]) is not int or not 0 <= row["msg_id"] <= 65535:
                    raise ValueError()
            if event in ("server_feedback_matched", "operation_verified") and row["phase"] != "succeeded":
                raise ValueError()
            facts[key].add(event)
            if event == "operation_verified":
                declared.add(key)
        except (ValueError, TypeError, KeyError):
            invalid += 1
    required = {"press_accepted", "release_accepted", "client_request_matched", "server_feedback_matched", "operation_verified"}
    rejected = {"press_rejected", "release_rejected", "client_request_rejected", "client_response_rejected", "server_feedback_conflict"}
    verified = {key for key in declared if key[1] > 0 and required <= facts[key] and not (rejected & facts[key])}
    ids = {key[0] for key in verified}
    return {"verified_ron": len({key[0] for key in verified if key[2] == "ron"}),
            "verified_tsumo": len({key[0] for key in verified if key[2] == "tsumo"}),
            "unverified_win_operations": len(wins - ids), "invalid_records": invalid}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    args = parser.parse_args()
    with args.log.open(encoding="utf-8") as source:
        print(json.dumps(audit_lines(source), sort_keys=True))


if __name__ == "__main__":
    main()
