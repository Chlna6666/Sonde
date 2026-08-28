#!/usr/bin/env python3
"""
Cloudflare D1 Export & Sonde Migration Tool
===========================================
This standalone script exports telemetry data from legacy Cloudflare D1 databases
and prepares/imports it for Sonde.
"""

import argparse
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
import time
import urllib.parse
import urllib.request
import uuid

def parse_sql_file(sql_file_path):
    print(f"[*] Parsing SQL file: {sql_file_path}")
    pattern = re.compile(
        r'INSERT\s+INTO\s+["`]?events["`]?\s*\((.*?)\)\s*VALUES\s*\((.*?)\);',
        re.IGNORECASE
    )
    rows = []
    with open(sql_file_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            m = pattern.match(line)
            if not m:
                continue
            cols = [c.strip().strip('"`') for c in m.group(1).split(",")]
            # Parse CSV values carefully
            val_str = m.group(2)
            # Custom simple tokenizer for SQL VALUES
            vals = []
            cur = ""
            in_quote = False
            for char in val_str:
                if char == "'":
                    in_quote = not in_quote
                    cur += char
                elif char == "," and not in_quote:
                    vals.append(cur.strip())
                    cur = ""
                else:
                    cur += char
            if cur:
                vals.append(cur.strip())

            row_dict = {}
            for col, val in zip(cols, vals):
                if val.upper() == "NULL":
                    row_dict[col] = None
                elif val.startswith("'") and val.endswith("'"):
                    row_dict[col] = val[1:-1].replace("''", "'")
                elif val.isdigit():
                    row_dict[col] = int(val)
                else:
                    row_dict[col] = val
            rows.append(row_dict)

    print(f"[+] Successfully parsed {len(rows)} INSERT statements from SQL file.")
    return rows

def direct_import_to_sonde(
    rows,
    sqlite_db_path,
    app_name="Migrated Application",
    app_slug="migrated-app",
    env_name="Production",
    env_slug="production",
    ingest_key=None,
):
    if not os.path.exists(sqlite_db_path):
        print(f"[-] Sonde database not found at {sqlite_db_path}")
        return

    print(f"[*] Connecting to Sonde SQLite database: {sqlite_db_path}")
    conn = sqlite3.connect(sqlite_db_path)
    cur = conn.cursor()
    now_ms = int(time.time() * 1000)

    # 1. Ensure application exists
    cur.execute("SELECT id FROM applications WHERE slug = ? LIMIT 1", (app_slug,))
    app_res = cur.fetchone()
    if app_res:
        app_id = app_res[0]
        print(f"[+] Using existing application: {app_name} ({app_id})")
    else:
        app_id = str(uuid.uuid4())
        cur.execute(
            "INSERT INTO applications (id, name, slug, retention_days, created_at) VALUES (?, ?, ?, ?, ?)",
            (app_id, app_name, app_slug, 90, now_ms)
        )
        print(f"[+] Created application: {app_name} ({app_id})")

    # 2. Ensure environment exists
    cur.execute("SELECT id FROM environments WHERE application_id = ? AND slug = ? LIMIT 1", (app_id, env_slug))
    env_res = cur.fetchone()
    if env_res:
        env_id = env_res[0]
        print(f"[+] Using existing environment: {env_name} ({env_id})")
    else:
        env_id = str(uuid.uuid4())
        cur.execute(
            "INSERT INTO environments (id, application_id, name, slug, created_at) VALUES (?, ?, ?, ?, ?)",
            (env_id, app_id, env_name, env_slug, now_ms)
        )
        print(f"[+] Created environment: {env_name} ({env_id})")

    # 3. Optionally register a supplied legacy ingest key. The migration itself
    # does not require a key, so none is persisted unless explicitly requested.
    if ingest_key:
        key_hash = hashlib.sha256(ingest_key.encode("utf-8")).hexdigest()
        cur.execute("SELECT id FROM api_keys WHERE key_hash = ? LIMIT 1", (key_hash,))
        if not cur.fetchone():
            key_id = str(uuid.uuid4())
            cur.execute(
                "INSERT INTO api_keys (id, application_id, environment_id, name, key_hash, key_prefix, scopes, expires_at, last_used_at, revoked_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (key_id, app_id, env_id, "Legacy Ingest Key", key_hash, ingest_key[:6], '["ingest"]', None, None, None, now_ms),
            )
            print("[+] Registered a supplied legacy API key.")

    # 4. Insert events
    inserted = 0
    deduped = 0

    print(f"[*] Importing {len(rows)} events into Sonde...")
    for r in rows:
        event_id = str(uuid.uuid4())
        ts = int(r.get("ts", now_ms))
        day = str(r.get("day", ""))
        user_hash = str(r.get("user_hash", ""))
        app_ver = r.get("app_version")
        launcher_ver = r.get("launcher_version")
        os_name = r.get("os")
        key_id = r.get("key_id")

        dedupe_key = f"d1:{app_id}:{day}:{user_hash}"

        cur.execute("SELECT id FROM events WHERE dedupe_key = ? LIMIT 1", (dedupe_key,))
        if cur.fetchone():
            deduped += 1
            continue

        attr = {
            "migration.source_day": day,
            "migration.source_timestamp": ts,
            "migration.source_user_hash": user_hash,
        }
        if key_id:
            attr["migration.source_key_id"] = key_id

        attributes_json = json.dumps(attr, ensure_ascii=False)

        cur.execute(
            """
            INSERT INTO events (id, application_id, environment_id, name, timestamp, day, anonymous_id, session_id, app_version, launcher_version, os, attributes, dedupe_key, received_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                event_id,
                app_id,
                env_id,
                "migration.application_start",
                ts,
                day,
                user_hash,
                None,
                app_ver,
                launcher_ver,
                os_name,
                attributes_json,
                dedupe_key,
                now_ms,
            )
        )
        inserted += 1

    # 5. Record import run
    source_hash = hashlib.sha256(f"direct_import_{now_ms}_{len(rows)}".encode()).hexdigest()
    cur.execute(
        """
        INSERT INTO import_runs (id, source_type, source_hash, application_id, environment_id, status, inserted, deduped, rejected, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            str(uuid.uuid4()),
            "d1",
            source_hash,
            app_id,
            env_id,
            "completed",
            inserted,
            deduped,
            0,
            now_ms,
        )
    )

    conn.commit()
    conn.close()
    print(f"\n[✓] Sonde Direct Import Finished:")
    print(f"    - Total Processed: {len(rows)}")
    print(f"    - Successfully Inserted: {inserted}")
    print(f"    - Deduplicated / Skipped: {deduped}")
    print(f"    - Target Application: {app_name} ({app_id})")
    print(f"    - Target Environment: {env_name} ({env_id})")

def main():
    parser = argparse.ArgumentParser(description="Cloudflare D1 Export & Sonde Migration Script")
    parser.add_argument("--sql-file", help="Path to exported SQL file", required=True)
    parser.add_argument("--sonde-db", help="Path to Sonde SQLite database", required=True)
    parser.add_argument("--app-name", help="Target application name", default="Migrated Application")
    parser.add_argument("--app-slug", help="Target application slug", default="migrated-app")
    parser.add_argument("--env-name", help="Target Environment Name", default="Production")
    parser.add_argument("--env-slug", help="Target Environment Slug", default="production")
    parser.add_argument(
        "--ingest-key",
        help="Optional legacy ingest key; defaults to SONDE_INGEST_KEY when set",
        default=os.environ.get("SONDE_INGEST_KEY"),
    )

    args = parser.parse_args()

    if not os.path.exists(args.sql_file):
        print(f"[-] SQL file not found at: {args.sql_file}")
        sys.exit(1)

    rows = parse_sql_file(args.sql_file)
    if rows and args.sonde_db:
        direct_import_to_sonde(
            rows,
            args.sonde_db,
            args.app_name,
            args.app_slug,
            args.env_name,
            args.env_slug,
            args.ingest_key,
        )

if __name__ == "__main__":
    main()
