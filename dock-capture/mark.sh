#!/bin/sh
# Append a timestamped marker to every capture file. Usage: mark.sh "text"
d=$(dirname "$0")
line="### MARK $(date '+%Y-%m-%d %H:%M:%S') $*"
for f in sampler.tsv syslog.log markers.log; do echo "$line" >> "$d/$f"; done
echo "$line"
