#!/bin/sh
# VOIDWRAP RUN launcher (Linux). Run:  ./play.sh          (just you)
#                                     ./play.sh friends  (friends on your network can join)
cd "$(dirname "$0")" || exit 1
if [ "$1" = "friends" ]; then
  echo "Friends on the same network open one of these in their browser:"
  for ip in $(hostname -I 2>/dev/null); do echo "    http://$ip:7878"; done
  export VOIDWRAP_HOST=0.0.0.0
fi
echo "VOIDWRAP RUN is starting - your browser will open http://localhost:7878"
echo "Keep this terminal open while you play. Press Ctrl+C to stop the game."
export VOIDWRAP_OPEN_BROWSER=1
exec ./voidwrap-server
