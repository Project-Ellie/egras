.PHONY: notebooks-up notebooks-test

notebooks-up:
	@echo "Manual sequence (see notebooks/README.md):"
	@echo "  docker-compose up postgres -d"
	@echo "  cargo run -- seed-admin --email \$$EGRAS_OPERATOR_EMAIL --username admin --password \$$EGRAS_OPERATOR_PASSWORD"
	@echo "  cargo run"

notebooks-test:
	pytest --nbmake notebooks/scenarios
