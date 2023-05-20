#!/bin/bash
make install-ubuntu-deps >> devcontainerInstall.log 2>&1
make start-deps >> devcontainerInstall.log 2>&1
