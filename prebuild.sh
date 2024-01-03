#!/usr/bin/env bash

if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev libgtk-3-dev
elif [[ "$OSTYPE" == "darwin"* ]]; then
fi
