# DOC-002: Unpinned base image tag
FROM python:latest

# DOC-006: Sensitive secret in ENV
ENV API_KEY=supersecret12345

# DOC-007: apt-get without clean
RUN apt-get update && apt-get install -y curl

# DOC-001: Source code copied before dependency installation
COPY . .

# DOC-008: pip install without --no-cache-dir
RUN pip install uvicorn

# DOC-012: ADD used for local files instead of COPY
ADD main.py /app/main.py

# DOC-013: sudo installed
RUN apt-get install sudo

# Web server keyword 'uvicorn' is present:
# DOC-004: Missing USER (runs as root)
# DOC-005: Missing HEALTHCHECK
# DOC-011: Web server without EXPOSE
# DOC-009: Missing ENV PYTHONUNBUFFERED=1
# DOC-010: Missing ENV PYTHONDONTWRITEBYTECODE=1
CMD ["uvicorn", "main:app"]
