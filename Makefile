.PHONY: notification create_notification

api:
	@echo "Running notification api"
	RUN_MODE=api cargo run
