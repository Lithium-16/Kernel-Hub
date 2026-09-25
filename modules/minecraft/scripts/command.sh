# Send one console command and print what the server logged in response.
# Needs: MC_DIR, CMD, WAIT_S.
log="$MC_DIR/logs/latest.log"
size() { stat -c %s "$log" 2>/dev/null || echo 0; }

before=$(size)
console_send "$CMD"
sleep "$WAIT_S"
after=$(size)
if [ "$after" -ge "$before" ]; then
  tail -c +"$((before + 1))" "$log"
else
  # The log rotated while we waited.
  tail -n 20 "$log"
fi
