# Start, stop or restart a systemd unit. `systemctl stop` waits for the unit to exit.
# Needs: UNIT, VERB.
systemctl "$VERB" "$UNIT"
echo "state=$(systemctl is-active "$UNIT" 2>/dev/null || true)"
