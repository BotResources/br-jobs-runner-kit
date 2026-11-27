FROM python:3.12-slim@sha256:191083cdf6c7bda8c76158983163ead5d5cf7b77cf3033e805bb0f5296f51b4b
RUN apt-get update \
    && apt-get install -y --no-install-recommends git cargo ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /work /home/agent && chmod 1777 /work /home/agent
ENV HOME=/home/agent
WORKDIR /work
