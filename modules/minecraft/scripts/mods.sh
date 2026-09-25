# Installed mod jars as "name<TAB>bytes". Needs: MC_DIR.
find "$MC_DIR/mods" -maxdepth 1 -name '*.jar' -printf '%f\t%s\n' | sort -f
