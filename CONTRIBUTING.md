# Contributing

When contributing to this repository, please first discuss the change you wish to make via issue,
email, or any other method with the owners of this repository before making a change.
Fix for bugs can be opened out of the blue

## Pull Request Process

All pull request should point to the main branch

## Dev enviroment setup
### github spaces development setup
[pricing](https://docs.github.com/en/billing/managing-billing-for-github-codespaces/about-billing-for-github-codespaces) at the time of this guide free tier has 120 hours of free use 

launch a new codepace here

![image](https://user-images.githubusercontent.com/4585690/236680399-36e2fc82-17fc-4b30-b914-686abbff191f.png)

then in the terminal do `make install-ubuntu-deps` which will install all the needed deps (follow the instruction if input is needed) 

make sure you open a new terminal before using other commands 

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

### set enviroment variables

edit the `.dev.env` file with your api keys if needed (be sure to not commit your secrets accidentally)

### launch required services (!!!REQUIRED!!!)
run `make start-deps`

you need to run at least once at the start of your development session

### launch tests
run `make test`

### launch the server
run `make run`

### automatically run the project when making changes
run `make hot-reload`

### build the container image locally
run `make image`
