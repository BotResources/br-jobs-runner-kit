FROM python:3.12-slim@sha256:191083cdf6c7bda8c76158983163ead5d5cf7b77cf3033e805bb0f5296f51b4b

LABEL org.opencontainers.image.source="https://github.com/BotResources/br-jobs-runner-kit" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.description="Test and reference image of the sandbox: Python 3.12, pandas, openpyxl, python-docx, python-pptx, headless LibreOffice"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libreoffice-core \
        libreoffice-writer \
        libreoffice-calc \
        libreoffice-impress \
        fonts-dejavu-core \
        procps \
    && rm -rf /var/lib/apt/lists/*

COPY images/br-sandbox-python/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir --root-user-action=ignore -r /tmp/requirements.txt \
    && rm /tmp/requirements.txt

RUN sed -i 's|^UserInstallation=.*|UserInstallation=file:///home/agent/.libreoffice|' /usr/lib/libreoffice/program/bootstraprc \
    && grep -q '^UserInstallation=file:///home/agent/.libreoffice$' /usr/lib/libreoffice/program/bootstraprc

RUN mkdir -p /work /home/agent \
    && chmod 1777 /work /home/agent

ENV HOME=/home/agent
WORKDIR /work
