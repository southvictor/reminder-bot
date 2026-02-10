
NotificationBot
===============

- Deploy reconciliation loop
- Add notification listener to service for reading from discord
- add a goal tracker

Architecture
------------

Components + Runtime Flow
```
                 ┌──────────────────────────────────────────────────┐
                 │                    runtime.rs                     │
                 └───────────────┬───────────────────────────────────┘
                                 │
                                 │ creates
                                 ▼
                     ┌─────────────────────┐
                     │   TaskRunner        │
                     └─────────┬───────────┘
                               │ starts
        ┌──────────────────────┼───────────────────────────────┐
        │                      │                               │
        ▼                      ▼                               ▼
┌─────────────────┐   ┌─────────────────┐              ┌─────────────────┐
│ notification    │   │ todo_loop       │              │ calendar_loop   │
│ loop            │   │                 │              │                 │
└─────────────────┘   └─────────────────┘              └─────────────────┘

                                 │
                                 │ sets up
                                 ▼
                      ┌────────────────────┐
                      │   EventBus         │
                      │   (mpsc channel)   │
                      └─────────┬──────────┘
                                │
                                │ spawn worker
                                ▼
                      ┌────────────────────┐
                      │  events/worker.rs  │
                      │  (run_event_worker)│
                      └─────────┬──────────┘
                                │ handles
                                ▼
        ┌──────────────────────────────────────────────┐
        │ Event::NotifyRequested (text/user/channel)   │
        │  - calls OpenAI (notification prompt)        │
        │  - builds PendingNotification + stores in map │
        │  - sends Discord pending message + buttons    │
        └──────────────────────────────────────────────┘

                                 ▲
                                 │ emits
                                 │
                      ┌─────────┴──────────┐
                      │ handlers/discord.rs│
                      │ (BotHandler)       │
                      └─────────┬──────────┘
                                │
                                │ receives
                                ▼
        ┌──────────────────────────────────────────────┐
        │ Discord interactions: /notify, buttons         │
        │ - /notify → routing/state machine (notify_flow)│
        │ - if notification → emit Event::NotifyRequested│
        │ - if todolist → create todo                    │
        │ - if unknown → prompt clarification            │
        └──────────────────────────────────────────────┘
```

Notify Flow State Machine
```
          ┌──────────────┐
          │    Idle      │
          └──────┬───────┘
                 │ /notify input
                 ▼
          ┌──────────────┐
          │  Routing     │  (IntentRouter: LLM w/ heuristic fallback)
          └───┬───────┬──┘
              │       │
    notification    todolist              unknown
              │       │                   │
              ▼       ▼                   ▼
   ┌────────────────┐ ┌────────────────┐ ┌─────────────────┐
   │ Pending         │ │ Todo Created   │ │ Unknown         │
   │ Notification    │ │ (saved)        │ │ (clarify prompt)│
   └──────┬─────────┘ └────────────────┘ └────────┬────────┘
          │ confirm/cancel                                   │ follow-up /notify
          ▼                                                  └──────────────┐
     ┌───────────┐                                                         ▼
     │ Confirmed │                                                   ┌──────────────┐
     └───────────┘                                                   │  Routing     │
                                                                     └──────────────┘
```

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

Configuration
-------------
The app reads configuration from `./config.properties` by default. You can override the path with the `CONFIG_FILE` environment variable.

Example `config.properties`:
```
DISCORD_CLIENT_SECRET=your-discord-bot-token
OPENAI_API_KEY=your-openai-api-key
DB_LOCATION=./data
RUN_MODE=api
```

Set DISCORD_CLIENT_SECRET to the discord app's bot token.
