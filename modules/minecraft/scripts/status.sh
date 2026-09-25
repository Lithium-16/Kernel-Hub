# One snapshot of the server, printed as key=value lines, then the status ping.
# Needs: MC_DIR, MC_SERVICE, BACKUP_DIR, SERVICES (space separated).
svc() { systemctl is-active "$1" 2>/dev/null || true; }

echo "now=$(date +%s)"
echo "state=$(svc "$MC_SERVICE")"
for s in $SERVICES; do echo "service.$s=$(svc "$s")"; done

since=$(systemctl show "$MC_SERVICE" -p ActiveEnterTimestamp --value 2>/dev/null || true)
if [ -n "$since" ] && [ "$since" != "n/a" ]; then
  echo "since=$(date -d "$since" +%s 2>/dev/null || true)"
fi
echo "memory=$(systemctl show "$MC_SERVICE" -p MemoryCurrent --value 2>/dev/null || true)"
echo "mods=$(find "$MC_DIR/mods" -maxdepth 1 -name '*.jar' 2>/dev/null | wc -l)"
find "$BACKUP_DIR" -maxdepth 1 -name '*.tar.gz' -printf 'backup=%T@ %s %f\n' 2>/dev/null | sort -t= -k2 -rn | head -5 || true

if [ "$(svc "$MC_SERVICE")" = "active" ]; then
  echo "--ping--"
  timeout 5 mc-ping 2>&1 || echo "--ping-failed--"
fi
