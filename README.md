
ReminderBot
===========

- Deploy reconciliation loop
- Add notification listener to service for reading from discord
- add a goal tracker

Docker build
------------

This repo includes a `Dockerfile` that builds a static Linux binary using `messense/rust-musl-cross` with musl:

From a parent directory cloning reminderBot and memory_db
- Build: `docker build -f reminderBot/Dockerfile -t reminderbot .`
- Copy linux binary:
  ```
  CID=$(docker create reminderbot)
  docker cp "$CID":/usr/local/bin/reminderBot ./reminderBot-linux
  docker rm "$CID"
  ```

Required Permissions for channel.
- bot
- applications.commands
- Send messages
- View Channels

Set DISCORD_CLIENT_SECRET to the discord app's bot token.