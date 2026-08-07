#! /bin/bash
# This should print the output of the services ordered by the first number
../target/debug/lksystem 2> /dev/null | grep ".service]" &

sleep 5
killall lksystem
sleep 1