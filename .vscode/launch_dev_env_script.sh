#!/bin/bash

if ! redis-cli ping > /dev/null; then
  # Redis is not running, start it with docker-compose
  if ! sudo docker-compose -f docker/docker-compose.dev_env.yml up -d; then
    echo "to run the dev enviroment check if docker service is up, and be sure to have redis-cli and docker-compose installed"
  fi
else
  # Redis is already running
  echo "Redis container is running"
fi
