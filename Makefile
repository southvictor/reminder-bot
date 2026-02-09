RUN_MODE ?= cli

.PHONY: notification create_notification

api:
	@echo "Running notification api"
	RUN_MODE=api cargo run

create_notification:
	RUN_MODE=cli cargo run -- create "remember to file your taxes" "${DISCORD_USER_ID}" "2020-01-01T12:00:00Z" "$(DISCORD_CHANNEL_ID)"
