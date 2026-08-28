PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE IF NOT EXISTS "events" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "ts" INTEGER NOT NULL,
  "day" TEXT NOT NULL,
  "user_hash" TEXT NOT NULL,
  "app_version" TEXT,
  "launcher_version" TEXT,
  "os" TEXT,
  "key_id" TEXT
);
INSERT INTO "events" ("ts", "day", "user_hash", "app_version", "launcher_version", "os", "key_id") VALUES
  (1787650000000, '2026-08-25', 'legacy-user-a', '2.1.0', '1.4.0', 'Windows', 'legacy'),
  (1787650000100, '2026-08-25', 'legacy-user-a', '2.1.0', '1.4.0', 'Windows', 'legacy'),
  (1787736400000, '2026-08-26', 'legacy-user-b', '2.1.1', '1.4.0', 'Linux', 'legacy');
COMMIT;
