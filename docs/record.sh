#!/usr/bin/env bash
# Records the README videos from docs/tapes with VHS (brew install vhs),
# on the demo workspace: `slackterm demo` needs no Slack account.
#
#   docs/record.sh            every tape
#   docs/record.sh hero       only docs/tapes/hero.tape
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release
mkdir -p docs/media target/vhs

tapes=("$@")
if [ ${#tapes[@]} -eq 0 ]; then
    tapes=(hero navigation search reactions layouts themes)
fi

for tape in "${tapes[@]}"; do
    echo "→ $tape"
    vhs "docs/tapes/$tape.tape"
done

# The four themes side by side, at half size.
if [[ " ${tapes[*]} " == *" themes "* ]]; then
    ffmpeg -loglevel error -y \
        -i target/vhs/theme-1.png -i target/vhs/theme-2.png \
        -i target/vhs/theme-3.png -i target/vhs/theme-4.png \
        -filter_complex "[0]scale=iw/2:-1[a];[1]scale=iw/2:-1[b];[2]scale=iw/2:-1[c];[3]scale=iw/2:-1[d];[a][b][c][d]xstack=inputs=4:layout=0_0|w0_0|0_h0|w0_h0" \
        docs/media/themes.png
fi
