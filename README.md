
ReminderBot
===========

- Deploy reconciliation loop
- Add notification listener to service for reading from discord
- add a goal tracker

Docker build
------------

This repo includes a `Dockerfile` that builds a static Linux binary using `messense/rust-musl-cross` with musl:

- Build: `docker build -f reminderBot/Dockerfile -t reminderbot .`

