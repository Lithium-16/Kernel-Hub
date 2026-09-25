# Read-only check of the assumptions this module makes. Run from the repo root on a machine
# with `ssh pluto`. PowerShell has no `<`, and piping with Get-Content can turn line endings
# into CRLF, so hand the redirect to cmd:
#   cmd /c 'ssh pluto "wsl -d Ubuntu -u root -- bash -s" < modules\minecraft\probe.sh'
# (bash/cmd: ssh pluto "wsl -d Ubuntu -u root -- bash -s" < modules/minecraft/probe.sh)
# It prints no secrets (no rcon password, no webhook).
set -u
echo "== tools"; for t in systemctl mc-cmd mc-ping mc-console tar gzip; do printf '%-10s %s\n' "$t" "$(command -v "$t" || echo MISSING)"; done
echo "== mc-cmd"; sed -n '1,40p' "$(command -v mc-cmd)" 2>/dev/null | grep -viE 'pass|secret|token|webhook'
echo "== mc-ping output"; timeout 5 mc-ping 2>&1 | head -c 1500; echo
echo "== units"; for u in minecraft playit mc-notify; do echo "$u: $(systemctl is-active "$u")"; done
systemctl cat minecraft 2>/dev/null | grep -E '^(Exec|User|Type|Restart|KillMode|TimeoutStopSec)'
echo "== server"; ls -la /srv/minecraft | head -30
grep -E '^(level-name|server-port|enable-rcon|max-players|white-list)=' /srv/minecraft/server.properties
tail -n 3 /srv/minecraft/logs/latest.log
echo "== disk"; df -h /srv | tail -1; du -sh /srv/minecraft/"$(sed -n 's/^level-name=//p' /srv/minecraft/server.properties)" 2>/dev/null
