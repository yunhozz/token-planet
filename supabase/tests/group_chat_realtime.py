#!/usr/bin/env python3
"""Explicitly approved scratch-only Auth/REST/Postgres Changes verification.

No dependencies, target overrides, credentials in argv, or raw diagnostics.
Only generated fixture IDs are cleaned; never starts/stops/resets a stack.
"""
import base64
import datetime
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import re
import select
import socket
import struct
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request
import uuid

API = "http://127.0.0.1:54321"
WORKDIR = Path("/tmp/token-planet-group-chat-supabase")
PROJECT = "token-planet-group-chat-qa"
CONTAINER = "supabase_db_" + PROJECT
TIMEOUT = 5
CHECKS = {
    "R01": "local_identity_publication_replica_identity",
    "R02": "independent_auth_users_and_invite_memberships",
    "R03": "rest_rpc_grants_and_private_read_state",
    "R04": "forged_send_and_foreign_delete_denied",
    "R05": "paused_server_profile_and_default_snapshot",
    "R06": "idempotent_request_and_changed_body_denial",
    "R07": "new_member_cutoff_rest_rpc_realtime",
    "R08": "nonmember_and_other_world_realtime_denial",
    "R09": "tombstone_update_old_payload_privacy",
    "R10": "offline_insert_tombstone_and_session_reconnect_recovery",
    "R11": "private_monotonic_read_cursor",
    "R12": "leave_and_rejoin_cutoff_all_transports",
    "R13": "membership_auth_deletion_preserves_messages",
    "R14": "last_owner_group_delete_cascades_chat",
    "R15": "local_websocket_transport_100_samples",
    "R16": "server_text_validation_and_plain_html_storage",
}
METRICS = {"transport_samples", "transport_server_to_ws_p95_ms", "transport_roundtrip_to_ws_p95_ms"}

class HarnessFailure(RuntimeError):
    pass

def require(value, code="check_failed"):
    if not value:
        raise HarnessFailure(code)

def validate_api(value):
    require(value == API, "local_endpoint_required")
    return API

def validate_environment(env):
    forbidden = {"DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_TLS_VERIFY", "DOCKER_CERT_PATH", "DATABASE_URL", "PGHOST", "PGHOSTADDR", "PGPORT", "PGDATABASE", "PGUSER", "PGPASSWORD", "PGSERVICE", "PGSERVICEFILE", "SUPABASE_ACCESS_TOKEN", "SUPABASE_PROJECT_ID", "TOKEN_PLANET_SUPABASE_URL", "TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"}
    require(not any(env.get(key) for key in forbidden), "connection_override_forbidden")

def validate_container(info, context):
    require(context.get("Endpoints", {}).get("docker", {}).get("Host", "").startswith("unix:///"), "local_docker_required")
    labels = info.get("Config", {}).get("Labels") or {}
    require(info.get("Name") == "/" + CONTAINER and info.get("State", {}).get("Running") and labels.get("com.supabase.cli.project") == PROJECT and Path(labels.get("com.supabase.cli.workdir", "/")).resolve() == WORKDIR.resolve() and re.fullmatch("[0-9a-f]{64}", info.get("Id", "")), "container_identity_mismatch")
    ports = info.get("NetworkSettings", {}).get("Ports", {}).get("5432/tcp")
    require(ports and all(p.get("HostPort") == "54322" and p.get("HostIp") in {"127.0.0.1", "0.0.0.0", "::"} for p in ports), "db_port_mismatch")
    return info["Id"]

def run_command(argv, stdin=None):
    try:
        result = subprocess.run(argv, input=stdin, capture_output=True, text=True, timeout=20)
    except subprocess.TimeoutExpired:
        raise HarnessFailure("command_timeout") from None
    require(result.returncode == 0, "command_failed")
    return result.stdout

def safe_error(error):
    # Static identifiers only. Never serialize arbitrary exception text or payloads.
    allowed = {"auth_fixture_failed", "world_fixture_failed", "verification_failed", "command_timeout", "command_failed", "http_failed", "ws_handshake_failed", "ws_join_denied", "ws_join_timeout", "ws_timeout", "ws_closed", "ws_message_timeout", "ws_closed_or_invalid", "ws_subscription_rejected", "ws_subscription_timeout", "transport_latency_unmet", "clock_measurement_invalid", "verification_deadline", "container_changed", "local_endpoint_required", "connection_override_forbidden", "scratch_config_mismatch", "api_container_mismatch", "explicit_run_required", "check_failed", "rpc_status_mismatch"}
    return str(error) if isinstance(error, HarnessFailure) and str(error) in allowed else "operation_failed"

class Results:
    def __init__(self):
        self.checks = {key: "unverified" for key in CHECKS}
        self.metrics = {}
    def record(self, check, passed):
        require(check in CHECKS and type(passed) is bool, "invalid_report_field")
        self.checks[check] = "proven" if passed else "unmet"
    def metric(self, key, value):
        require(key in METRICS and type(value) in (int, float) and math.isfinite(value) and value >= 0, "invalid_report_metric")
        self.metrics[key] = round(value, 3)
    def report(self):
        return {"target": PROJECT, "api": API, "checks": {key: {"name": CHECKS[key], "status": status} for key, status in self.checks.items()}, "metrics": self.metrics}

def percentile95(samples):
    require(bool(samples), "samples_required")
    return sorted(samples)[math.ceil(len(samples) * .95) - 1]

def latency_goal_met(samples):
    require(len(samples) >= 100, "samples_required")
    return percentile95(samples) <= 1000

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        raise HarnessFailure("redirect_forbidden")

class LocalTarget:
    def __init__(self):
        validate_environment(os.environ)
        config = tomllib.loads((WORKDIR / "supabase/config.toml").read_text())
        require(config.get("project_id") == PROJECT and config.get("realtime", {}).get("enabled") is True and config.get("api", {}).get("port") == 54321 and config.get("db", {}).get("port") == 54322, "scratch_config_mismatch")
        context = json.loads(run_command(["docker", "context", "inspect"]))[0]
        info = json.loads(run_command(["docker", "inspect", CONTAINER]))[0]
        self.container_id = validate_container(info, context)
        kong = json.loads(run_command(["docker", "inspect", "supabase_kong_" + PROJECT]))[0]
        require(kong.get("State", {}).get("Running") and kong.get("Config", {}).get("Labels", {}).get("com.supabase.cli.project") == PROJECT and any(p.get("HostPort") == "54321" for p in kong.get("NetworkSettings", {}).get("Ports", {}).get("8000/tcp", [])), "api_container_mismatch")
        status = json.loads(run_command(["npx", "--yes", "supabase@2.118.0", "status", "--workdir", str(WORKDIR), "-o", "json"]))
        validate_api(status.get("API_URL"))
        self.key = status.get("PUBLISHABLE_KEY")
        require(isinstance(self.key, str) and self.key.strip(), "publishable_key_missing")
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    def sql(self, statement):
        info = json.loads(run_command(["docker", "inspect", CONTAINER]))[0]
        require(info["Id"] == self.container_id, "container_changed")
        return run_command(["docker", "exec", "-i", self.container_id, "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "postgres"], statement).strip()
    def http(self, path, token=None, body=None, method=None, extra=None):
        require(path.startswith(("/auth/v1/", "/rest/v1/")) and not path.startswith("//"), "invalid_local_path")
        headers = {"apikey": self.key, "Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        headers.update(extra or {})
        request = urllib.request.Request(API + path, data=json.dumps(body).encode() if body is not None else None, headers=headers, method=method)
        try:
            with self.opener.open(request, timeout=TIMEOUT) as response:
                data = response.read(1_000_001)
                require(len(data) <= 1_000_000, "http_response_too_large")
                return response.status, json.loads(data) if data else None
        except urllib.error.HTTPError as error:
            # An expected denial is status-only. Never consume/print its body.
            status = error.code
            error.close()
            return status, None
        except (OSError, ValueError, http.client.HTTPException):
            raise HarnessFailure("http_failed") from None
    def rpc(self, user, name, args, denied=False):
        status, value = self.http("/rest/v1/rpc/" + name, user.get("access_token") if user else None, args, "POST")
        require(400 <= status < 500 if denied else 200 <= status < 300, "rpc_status_mismatch")
        return value

def checked_uuid(value):
    require(isinstance(value, str), "invalid_fixture_id")
    return str(uuid.UUID(value))

def subscription_ready(frame):
    payload = frame.get("payload", {})
    return frame.get("event") == "system" and payload.get("extension") == "postgres_changes" and payload.get("status") == "ok"

class Realtime:
    def __init__(self, target, user, world):
        self.socket = socket.create_connection(("127.0.0.1", 54321), timeout=TIMEOUT)
        self.buffer = bytearray()
        self.ref = 0
        self.closed = False
        self.last_heartbeat = time.monotonic()
        self.topic = "realtime:chat-qa-" + uuid.uuid4().hex
        self.pending = []
        nonce = base64.b64encode(os.urandom(16)).decode()
        path = "/realtime/v1/websocket?apikey=" + urllib.parse.quote(target.key, safe="") + "&vsn=1.0.0"
        self.socket.sendall((f"GET {path} HTTP/1.1\r\nHost: 127.0.0.1:54321\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {nonce}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
        while b"\r\n\r\n" not in self.buffer:
            chunk = self.socket.recv(4096)
            require(chunk and len(self.buffer) < 16384, "ws_handshake_failed")
            self.buffer.extend(chunk)
        header, tail = bytes(self.buffer).split(b"\r\n\r\n", 1)
        expected = base64.b64encode(hashlib.sha1((nonce + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest())
        require(header.startswith(b"HTTP/1.1 101") and expected in header, "ws_handshake_failed")
        self.buffer = bytearray(tail)
        ref = self.send("phx_join", {"config": {"broadcast": {"self": False}, "presence": {"key": ""}, "postgres_changes": [{"event": event, "schema": "public", "table": "group_chat_messages", "filter": "world_id=eq." + world} for event in ["INSERT", "UPDATE"]]}, "access_token": user.get("access_token", target.key)})
        deadline = time.monotonic() + TIMEOUT
        while time.monotonic() < deadline:
            frame = self.receive(deadline)
            if frame.get("event") == "phx_reply" and frame.get("ref") == ref:
                require(frame.get("payload", {}).get("status") == "ok", "ws_join_denied")
                break
        else:
            raise HarnessFailure("ws_join_timeout")
        # Realtime reports subscription readiness after the Phoenix join reply.
        # A denied role may receive a system error; actual row suppression is checked.
        ready = False
        deadline = time.monotonic() + TIMEOUT
        while time.monotonic() < deadline:
            frame = self.receive(deadline)
            if subscription_ready(frame):
                ready = True
                break
            if frame.get("event") == "system" and frame.get("payload", {}).get("extension") == "postgres_changes" and frame["payload"].get("status") == "error":
                # Nonmembers still join the table channel; RLS suppression is tested separately.
                raise HarnessFailure("ws_subscription_rejected")
            if frame.get("event") == "postgres_changes":
                self.pending.append(frame["payload"]["data"])
        require(ready, "ws_subscription_timeout")
    def send_frame(self, payload, opcode=1):
        mask = os.urandom(4)
        size = len(payload)
        header = bytes([0x80 | opcode, 0x80 | size]) if size < 126 else bytes([0x80 | opcode, 0x80 | 126]) + struct.pack("!H", size)
        self.socket.sendall(header + mask + bytes(value ^ mask[i % 4] for i, value in enumerate(payload)))
    def tick(self, now=None):
        now = time.monotonic() if now is None else now
        if not self.closed and now - self.last_heartbeat >= 15:
            self.send("heartbeat", {}, topic="phoenix")
            self.last_heartbeat = now
    def send(self, event, payload, topic=None):
        self.ref += 1
        ref = str(self.ref)
        self.send_frame(json.dumps({"topic": topic or self.topic, "event": event, "payload": payload, "ref": ref}).encode())
        return ref
    def exact(self, size, deadline):
        while len(self.buffer) < size:
            remaining = deadline - time.monotonic()
            require(remaining > 0, "ws_timeout")
            self.socket.settimeout(remaining)
            try:
                chunk = self.socket.recv(4096)
            except TimeoutError:
                raise HarnessFailure("ws_timeout") from None
            require(chunk, "ws_closed")
            self.buffer.extend(chunk)
        value = bytes(self.buffer[:size]); del self.buffer[:size]
        return value
    def receive(self, deadline):
        while True:
            self.tick()
            head = self.exact(2, deadline)
            opcode, size = head[0] & 15, head[1] & 127
            require(head[0] & 0x80 and not head[1] & 0x80, "unsupported_ws_frame")
            if size == 126: size = struct.unpack("!H", self.exact(2, deadline))[0]
            if size == 127: size = struct.unpack("!Q", self.exact(8, deadline))[0]
            require(size <= 1_000_000, "ws_frame_too_large")
            body = self.exact(size, deadline)
            if opcode == 9: self.send_frame(body, 10); continue
            require(opcode == 1, "ws_closed_or_invalid")
            return json.loads(body)
    def changes(self, seconds):
        values = self.pending; self.pending = []
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.tick()
            if not self.buffer and not select.select([self.socket], [], [], max(0, deadline - time.monotonic()))[0]: break
            try:
                frame = self.receive(deadline)
            except HarnessFailure as error:
                if str(error) == "ws_timeout": break
                raise
            if frame.get("event") == "postgres_changes": values.append(frame["payload"]["data"])
        return values
    def message(self, message_id, event="INSERT"):
        deadline = time.monotonic() + TIMEOUT
        pending = self.pending; self.pending = []
        for value in pending:
            if value.get("record", {}).get("id") == message_id and value.get("type") == event:
                return value
        while time.monotonic() < deadline:
            frame = self.receive(deadline)
            if frame.get("event") == "postgres_changes":
                value = frame["payload"]["data"]
                if value.get("record", {}).get("id") == message_id and value.get("type") == event:
                    return value
        raise HarnessFailure("ws_message_timeout")
    def close(self):
        self.closed = True
        self.socket.close()

class Verification:
    def __init__(self, target, results):
        self.target, self.results = target, results
        self.users, self.worlds, self.sockets = [], [], []
        self.run_name = "chatqa_" + uuid.uuid4().hex
        self.deadline = time.monotonic() + 300
    def user(self):
        status, user = self.target.http("/auth/v1/signup", body={}, method="POST")
        require(status == 200 and user and user.get("access_token") and user.get("user", {}).get("id"), "auth_fixture_failed")
        checked_uuid(user["user"]["id"])
        self.users.append(user)
        return user
    def world(self, user):
        status, _ = self.target.http("/rest/v1/worlds", user["access_token"], {"owner_id": user["user"]["id"], "name": self.run_name, "timezone": "UTC"}, "POST", {"Prefer": "return=minimal"})
        require(status == 201, "world_fixture_failed")
        status, rows = self.target.http("/rest/v1/worlds?name=eq." + self.run_name, user["access_token"])
        require(status == 200 and len(rows) == 1, "world_fixture_failed")
        world = checked_uuid(rows[0]["id"]); self.worlds.append(world)
        return world
    def join(self, owner, user, world):
        invite = self.target.rpc(owner, "create_world_invite", {"p_world_id": world})[0]
        rows = self.target.rpc(user, "accept_world_invite", {"p_code": invite["code"]})
        require(rows[0]["status"] == "accepted" and rows[0]["world_id"] == world)
    def socket(self, user, world):
        connection = Realtime(self.target, user, world)
        self.sockets.append(connection)
        return connection
    def send(self, user, world, body="synthetic fixture", request=None):
        require(time.monotonic() < self.deadline, "verification_deadline")
        for connection in self.sockets:
            connection.tick()
        return self.target.rpc(user, "send_group_chat_message", {"p_world_id": world, "p_request_id": request or str(uuid.uuid4()), "p_body": body})
    def direct(self, user, world):
        status, rows = self.target.http("/rest/v1/group_chat_messages?world_id=eq." + world, user["access_token"])
        require(status == 200)
        return rows
    def listing(self, user, world, before=None):
        return self.target.rpc(user, "list_group_chat_messages", {"p_world_id": world, "p_before_seq": before, "p_limit": 50})
    def context(self, user, world):
        return self.target.rpc(user, "get_group_chat_context", {"p_world_id": world})
    def delete(self, user, world, message):
        return self.target.rpc(user, "delete_group_chat_message", {"p_world_id": world, "p_message_id": message["id"]})
    def check(self, key, action):
        try:
            action()
        except Exception as failure:
            self.results.record(key, False)
            raise HarnessFailure(safe_error(failure)) from None
        self.results.record(key, True)
    def run(self):
        t = self.target
        self.check("R01", lambda: require(self.publication_ok()))
        self.results.record("R02", False)
        a, b, c, d = [self.user() for _ in range(4)]
        require(len({u["user"]["id"] for u in self.users}) == 4)
        world = self.world(a); other = self.world(d); self.join(a, b, world)
        self.results.record("R02", True)
        def access():
            for user in [c, d]:
                require(self.direct(user, world) == [])
                t.rpc(user, "get_group_chat_context", {"p_world_id": world}, denied=True)
            t.rpc(None, "get_group_chat_context", {"p_world_id": world}, denied=True)
            status, _ = t.http("/rest/v1/group_chat_messages?world_id=eq." + world)
            require(400 <= status < 500)
            for method, body in [("POST", {"world_id": world, "body": "synthetic"}), ("PATCH", {"body": "synthetic"}), ("DELETE", None)]:
                status, _ = t.http("/rest/v1/group_chat_messages?world_id=eq." + world, a["access_token"], body, method)
                require(400 <= status < 500)
            for user in [a, b]:
                status, _ = t.http("/rest/v1/group_chat_reads", user["access_token"])
                require(400 <= status < 500)
                status, _ = t.http("/rest/v1/group_chat_reads", user["access_token"], extra={"Accept-Profile": "private"})
                require(400 <= status < 500)
        self.check("R03", access)
        bs, cs, ds = [self.socket(user, world) for user in [b, c, d]]
        # Fixed synthetic row only: no source transcripts, wallet, or user data.
        au = checked_uuid(a["user"]["id"])
        t.sql(f"insert into public.planet_member_state(user_id,nickname,avatar,timezone,current_cycle_id,cycle_started_at,current_planet_tokens,lifetime_tokens,growth_credit,stage,progress_to_next,incomplete,shared_visible) values ('{au}','QA profile','feminine','UTC','qa',now(),4242,4242,0,0,0,true,false);")
        first = self.send(a, world); bs.message(first["id"])
        own_request = str(uuid.uuid4())
        own = self.send(b, world, request=own_request); bs.message(own["id"])
        def profiles():
            require(first["nickname"] == "QA profile" and first["avatar"] == "feminine")
            require(own["nickname"] == "행성 동기화 대기" and own["avatar"] == "masculine")
            require(first["author_key"] not in {u["user"]["id"] for u in self.users})
            require(set(first) == {"id", "world_id", "message_seq", "change_seq", "author_key", "nickname", "avatar", "body", "created_at", "deleted_at"})
            require(all(row["current_planet_tokens"] == 0 and row["lifetime_tokens"] == 0 for row in t.rpc(b, "get_world_planets", {"p_world_id": world})))
            t.rpc(a, "delete_synced_usage", {"p_world_id": world})
            after = self.send(a, world); bs.message(after["id"])
            require(after["nickname"] == first["nickname"] and after["avatar"] == first["avatar"])
            require(self.listing(a, world)["messages"])
            t.sql(f"update public.planet_member_state set nickname='QA changed',avatar='masculine' where user_id='{au}';")
            require(any(row["id"] == first["id"] and row["nickname"] == first["nickname"] and row["avatar"] == first["avatar"] for row in self.listing(a, world)["messages"]))
        self.check("R05", profiles)
        def forged():
            args = {"p_world_id": world, "p_request_id": str(uuid.uuid4()), "p_body": "synthetic"}
            for key, value in [("p_author_id", b["user"]["id"]), ("p_profile", {"nickname": "forged", "avatar": "masculine"}), ("p_joined_after_seq", 0), ("p_created_at", "2026-01-01T00:00:00Z")]:
                t.rpc(a, "send_group_chat_message", {**args, key: value}, denied=True)
            t.rpc(d, "send_group_chat_message", args, denied=True)
            t.rpc(b, "delete_group_chat_message", {"p_world_id": world, "p_message_id": first["id"]}, denied=True)
        self.check("R04", forged)
        def retry():
            again = self.send(b, world, request=own_request)
            require(again["id"] == own["id"])
            require(sum(row["id"] == own["id"] for row in self.direct(a, world)) == 1)
            t.rpc(b, "send_group_chat_message", {"p_world_id": world, "p_request_id": own_request, "p_body": "changed synthetic"}, denied=True)
        self.check("R06", retry)
        self.join(a, c, world)
        def cutoff():
            require(self.listing(c, world)["messages"] == [] and self.direct(c, world) == [])
            require(self.listing(c, world, self.context(c, world)["joined_after_seq"])["messages"] == [])
            deleted = self.delete(a, world, first); update = bs.message(first["id"], "UPDATE")
            require(deleted["body"] is None and deleted["deleted_at"] is not None)
            require(not cs.changes(.8) and not ds.changes(.8))
            require(self.context(c, world)["unread_count"] == "0")
            limit = self.context(c, world)["last_change_seq"]
            require(t.rpc(c, "sync_group_chat_changes", {"p_world_id": world, "p_after_change_seq": "0", "p_until_change_seq": limit, "p_limit": 50})["messages"] == [])
            require(update.get("old_record", {}).get("body") is None and update["record"]["body"] is None)
            require(self.delete(a, world, first)["change_seq"] == deleted["change_seq"])
        self.check("R07", cutoff)
        self.results.record("R09", True)
        post = self.send(a, world); bs.message(post["id"]); cs.message(post["id"])
        def realtime_denial():
            require(not ds.changes(.8))
            own_other = self.send(d, other)
            require(not bs.changes(.5) and not cs.changes(.5))
            require(self.direct(b, other) == [])
            t.rpc(b, "list_group_chat_messages", {"p_world_id": other}, denied=True)
            require(self.listing(d, other)["messages"][0]["id"] == own_other["id"])
        self.check("R08", realtime_denial)
        watermark = self.context(b, world)["last_change_seq"]
        bs.close()
        offline = self.send(a, world); tombstone = self.delete(a, world, post)
        def recover():
            context = self.context(b, world)
            changes = t.rpc(b, "sync_group_chat_changes", {"p_world_id": world, "p_after_change_seq": watermark, "p_until_change_seq": context["last_change_seq"], "p_limit": 50})["messages"]
            require(any(row["id"] == offline["id"] for row in changes))
            require(any(row["id"] == tombstone["id"] and row["body"] is None for row in changes))
            require(any(row["id"] == post["id"] and row["body"] is None for row in self.listing(b, world)["messages"]))
        self.check("R10", recover)
        bs = self.socket(b, world)
        def read():
            current = self.context(b, world)
            require(int(current["unread_count"]) > 0)
            state = t.rpc(b, "mark_group_chat_read", {"p_world_id": world, "p_message_seq": current["last_message_seq"]})
            require(state["unread_count"] == "0")
            t.rpc(b, "mark_group_chat_read", {"p_world_id": world, "p_message_seq": "0"}, denied=True)
            require(set(self.context(a, world)) == {"world_id", "joined_after_seq", "author_key", "last_read_seq", "last_message_seq", "last_change_seq", "unread_count"})
        self.check("R11", read)
        def latency():
            server_to_ws, roundtrip = [], []
            for _ in range(100):
                began = time.monotonic()
                message = self.send(a, world)
                bs.message(message["id"])
                received = time.time()
                stored = datetime.datetime.fromisoformat(message["created_at"].replace("Z", "+00:00")).timestamp()
                require(received >= stored, "clock_measurement_invalid")
                server_to_ws.append((received - stored) * 1000)
                roundtrip.append((time.monotonic() - began) * 1000)
            self.results.metric("transport_samples", len(server_to_ws))
            self.results.metric("transport_server_to_ws_p95_ms", percentile95(server_to_ws))
            self.results.metric("transport_roundtrip_to_ws_p95_ms", percentile95(roundtrip))
            require(latency_goal_met(server_to_ws), "transport_latency_unmet")
        self.check("R15", latency)
        def text():
            for body in ["", " \n\t", "😀" * 2001]:
                t.rpc(a, "send_group_chat_message", {"p_world_id": world, "p_request_id": str(uuid.uuid4()), "p_body": body}, denied=True)
            boundary = self.send(a, world, "😀" * 2000); bs.message(boundary["id"])
            html = self.send(a, world, "<script>synthetic</script>"); bs.message(html["id"])
            require(html["body"] == "<script>synthetic</script>")
        self.check("R16", text)
        # Drain C's permitted traffic before checking leave boundaries.
        def rejoin():
            cs.changes(.2); ds.changes(.2); bs.changes(.2)
            require(t.rpc(b, "leave_world", {"p_world_id": world}) is True)
            require(any(row["id"] == own["id"] for row in self.direct(a, world)))
            after = self.send(a, world); cs.message(after["id"])
            require(not bs.changes(.8) and not ds.changes(.3))
            require(self.direct(b, world) == [])
            t.rpc(b, "get_group_chat_context", {"p_world_id": world}, denied=True)
            self.join(a, b, world)
            require(self.listing(b, world)["messages"] == [] and self.direct(b, world) == [])
            t.rpc(b, "send_group_chat_message", {"p_world_id": world, "p_request_id": own_request, "p_body": "synthetic fixture"}, denied=True)
            new = self.send(a, world); bs.message(new["id"]); cs.message(new["id"])
            require([row["id"] for row in self.listing(b, world)["messages"]] == [new["id"]])
        self.check("R12", rejoin)
        def retention():
            t.rpc(b, "leave_world", {"p_world_id": world})
            bu = checked_uuid(b["user"]["id"])
            t.sql(f"delete from auth.users where id='{bu}';")
            require(any(row["id"] == own["id"] and row["nickname"] == own["nickname"] for row in self.direct(a, world)))
            require(t.sql(f"select count(*) from private.group_chat_authors where user_id='{bu}';") == "0")
        self.check("R13", retention)
        def cascade():
            t.rpc(c, "leave_world", {"p_world_id": world})
            require(t.rpc(a, "leave_world", {"p_world_id": world}) is True)
            for table in ["public.group_chat_messages", "private.group_chat_state", "private.group_chat_authors", "private.group_chat_reads", "private.group_chat_requests"]:
                require(t.sql(f"select count(*) from {table} where world_id='{checked_uuid(world)}';") == "0")
        self.check("R14", cascade)
    def publication_ok(self):
        value = json.loads(self.target.sql("select json_build_object('published',(select array_agg(tablename order by tablename) from pg_publication_tables where pubname='supabase_realtime' and (tablename like 'group_chat_%' or schemaname='private')),'replica',(select relreplident from pg_class where oid='public.group_chat_messages'::regclass));"))
        return value["published"] == ["group_chat_messages"] and value["replica"] == "d"
    def cleanup(self):
        for connection in self.sockets:
            connection.close()
        if self.worlds:
            ids = ",".join("'" + checked_uuid(world) + "'" for world in self.worlds)
            self.target.sql(f"delete from public.worlds where id in ({ids}) and name='{self.run_name}';")
        if self.users:
            ids = ",".join("'" + checked_uuid(user["user"]["id"]) + "'" for user in self.users)
            self.target.sql(f"delete from auth.users where id in ({ids});")

def main():
    results = Results()
    verification = None
    error = None
    try:
        require(sys.argv[1:] == ["--run"], "explicit_run_required")
        target = LocalTarget()
        verification = Verification(target, results)
        verification.run()
    except Exception as failure:
        error = safe_error(failure)
    finally:
        if verification:
            try:
                verification.cleanup()
            except Exception as failure:
                error = safe_error(failure)
    report = results.report()
    if error: report["error"] = error
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 1 if error else 0

if __name__ == "__main__": sys.exit(main())
