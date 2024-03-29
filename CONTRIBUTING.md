# Contributing

When contributing to this repository, please first discuss the change you wish to make via issue,
email, or any other method with the owners of this repository before making a change.

Fix for small bugs or typos can be opened out of the blue as long as they don't alter the flow of the program too much.

When adding a feature try to also add some test if possible.

## Pull Request Process

All pull request should point to the main branch and should pass all tests (`make test`),
you are allowed to edit the tests if they stop having a meaning after your changes.

## How does it work?

vod2pod-rss is composed of 3 different phases:

* feed generation
* rewriting of the generated feed to inject the transcoded enabled urls
* live transcoding of the videos with ffmpeg

feed generation is done by implementing the `MediaProvider` trait which describes the generation of the raw rss feed, it's very important that
the provider generated a duration for each entry or seeking will not work.

this is then passed to the feed injection that will prepare the generic rss feed with the necessary links to the trancode endpoint.
one key rule is that the MediaProvider has no idea that the generated feed is intended for trancoding, this way if you disable trancoding
you still endup with a valid feed.

once a client makes a request it goes through the transcoding endpoint it will start a live trancode with ffmpeg and return the transcoded stream

optimization: most calls are cached with redis

### How to make a new provider

in the provider folder you will find the list of supported provider, to make a new one
add a file for your provider, implement the MediaProvider trait (you can copy generic.rs
to get a boilerplate) and then add it to provider/mod.rs inside the macro call

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

after launching the deps install script edit the `.env` file that was generated in the root folder with your api keys if needed (be sure to not commit your secrets accidentally)

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
