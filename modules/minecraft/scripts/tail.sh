# Last lines of the server log. Needs: MC_DIR, LINES.
tail -n "$LINES" "$MC_DIR/logs/latest.log" 2>/dev/null || true
