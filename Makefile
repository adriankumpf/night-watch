.PHONY: help build publish
help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' Makefile | \
	sort | \
	awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

build: ## Build docker image with the release
	@docker build --pull \
		-t adriankumpf/night-watch \
		-t ghcr.io/adriankumpf/night-watch:latest \
		.

publish: ## Publish docker image to the GitHub Container Registry
	@docker push ghcr.io/adriankumpf/night-watch:latest
