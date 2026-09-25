# Back up the world, verify the new archive, and only then delete old ones.
# Needs: MC_DIR, MC_SERVICE, BACKUP_DIR, KEEP, SAVE_TIMEOUT_S.
log="$MC_DIR/logs/latest.log"
size() { stat -c %s "$log" 2>/dev/null || echo 0; }
fail() { echo "error=$1"; exit 1; }

level=$(sed -n 's/^level-name=//p' "$MC_DIR/server.properties" 2>/dev/null | tr -d '\r')
level=${level:-world}
[ -f "$MC_DIR/$level/level.dat" ] || fail "no world at $MC_DIR/$level"
mkdir -p "$BACKUP_DIR"

need=$(du -sb "$MC_DIR/$level" | cut -f1)
avail=$(df --output=avail -B1 "$BACKUP_DIR" | tail -n 1 | tr -d ' ')
[ "$avail" -gt "$need" ] || fail "not enough disk space: need $need bytes, $avail free"

running=0
if systemctl is-active --quiet "$MC_SERVICE"; then running=1; fi
if [ "$running" = 1 ]; then
  # Stop autosaves and flush, then wait until the server says the save is done.
  trap 'mc-cmd "save-on" >/dev/null 2>&1 || true' EXIT
  before=$(size)
  mc-cmd "save-off" >/dev/null
  mc-cmd "save-all flush" >/dev/null
  saved=0
  for _ in $(seq 1 "$SAVE_TIMEOUT_S"); do
    # Not `tail | grep -q`: under pipefail, grep exiting early would fail the pipeline.
    if grep -q "Saved the game" < <(tail -c +"$((before + 1))" "$log" 2>/dev/null); then saved=1; break; fi
    sleep 1
  done
  [ "$saved" = 1 ] || fail "the server did not finish saving within ${SAVE_TIMEOUT_S}s"
fi

name="$level-$(date -u +%Y%m%dT%H%M%SZ).tar.gz"
tmp="$BACKUP_DIR/.$name.partial"
rm -f -- "$tmp"
tar -C "$MC_DIR" -czf "$tmp" -- "$level" || { rm -f -- "$tmp"; fail "tar failed"; }

if [ "$running" = 1 ]; then
  mc-cmd "save-on" >/dev/null || true
  trap - EXIT
fi

# Verify: the archive reads end to end and level.dat is intact gzip (NBT).
if ! tar -tzf "$tmp" >/dev/null || ! tar -xzOf "$tmp" -- "$level/level.dat" | gzip -t; then
  rm -f -- "$tmp"
  fail "the new backup did not verify; old backups were kept"
fi
mv -- "$tmp" "$BACKUP_DIR/$name"
echo "file=$name"
echo "bytes=$(stat -c %s "$BACKUP_DIR/$name")"

# Keep the newest $KEEP backups of this world.
find "$BACKUP_DIR" -maxdepth 1 -name "$level-*.tar.gz" -printf '%T@ %f\n' | sort -rn | cut -d' ' -f2- |
  tail -n +"$((KEEP + 1))" | while IFS= read -r old; do
    rm -f -- "$BACKUP_DIR/$old"
    echo "deleted=$old"
  done
