# Contributing

When contributing to this repository, please first discuss the change you wish to make via issue,
email, or any other method with the owners of this repository before making a change.

Fix for bugs can be opened out of the blue.

When adding a feature try to also add some test if possible.

## Pull Request Process

All pull request should point to the main branch and should pass all tests (`make test`)

## Initial setup
### github spaces development setup
[pricing](https://docs.github.com/en/billing/managing-billing-for-github-codespaces/about-billing-for-github-codespaces) 

At the time of this guide, the free tier offers 60 hours of free usage on a standard 2 core machine

launch a new codepace here

![image](https://user-images.githubusercontent.com/4585690/236680399-36e2fc82-17fc-4b30-b914-686abbff191f.png)

wait for the post script to end and your good to go (be sure to close and reopen the terminal or most commands will fail)

you will still need to add your apikeys to the .dev.env file and run `make start-deps` to have all features enabled

### local development setup
#### Requirements

you need to meake sure those commands are installed on your machine
- [docker](https://docs.docker.com/desktop/install/linux-install/)
- redis-cli (`sudo apt install redis`)
- ffmpeg (`sudo apt install ffmpeg`)
- yt-dlp (`sudo apt install python3-pip && pip3 install yt-dlp`)
- cargo ([rustup](https://rustup.rs/))
- Editor setup for rust development, for [VScode here is a guide](https://code.visualstudio.com/docs/languages/rust)

you can install the deps automatically on fedora and ubuntu with

`make install-fedora-deps`

or

`make install-ubuntu-deps`

**You will need to install Docker separately**
## General development commands

### set enviroment variables

edit the `.dev.env` file with your api keys if needed (be sure to not commit your secrets accidentally)

### launch required services (!!!REQUIRED!!!)
run `make start-deps`

you need to run at least once at the start of your development session (in codespaces this is done automatically)

### launch tests
run `make test`

### launch the server
run `make run`

### automatically run the project when making changes
run `make hot-reload`

### build the container image locally
run `make image`
