#!/bin/bash

tmux new-session -d -s my_session
tmux new-window -n 'nvim' 'nvim'
tmux new-window -n 'redis-cli' 'redis-cli'
tmux new-window -n 'cargo watch' 'cargo watch -x run'
tmux kill-window -t 0
tmux select-window -t 1
tmux attach-session -t my_session
