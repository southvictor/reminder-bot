.PHONY: notification create_notification

api:
	@echo "Running notification api"
	CONFIG_FILE=./local.dev.config.properties RUN_MODE=api cargo run
