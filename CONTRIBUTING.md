# Contributing

When contributing to this repository, please first discuss the change you wish to make via issue,
email, or any other method with the owners of this repository before making a change.
Fix for bugs can be opened out of the blue

## Pull Request Process

All pull request should point to the main branch

## Dev enviroment setup
### development setup
#### Requirements

you need to meake sure this commands are installed on you machine
- [docker](https://docs.docker.com/desktop/install/linux-install/) 
- redis-cli (`sudo apt install redis-cli`)
- ffmpeg (`sudo apt install ffmpeg`)
- yt-dlp (`sudo apt install python3-pip && pip3 install yt-dlp`) 
- cargo ([rustup](https://rustup.rs/))
- Editor setup for rust development, for [VScode here is a guide](https://code.visualstudio.com/docs/languages/rust)

#### set enviroment variables

edit the `.dev.env` file with your api keys if needed

#### launch required services (!!!REQUIRED!!!)
run `make start-deps` in the root of the project

#### launch tests
run `make test`

#### launch the server
run `make run`

#### automatically run the project when making changes
run `make hot-reload`

#### build the container image locally
run `make image`
