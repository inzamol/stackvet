import os

# Clean Django settings: environment variables
DEBUG = os.getenv("DJANGO_DEBUG", "False").lower() == "true"
SECRET_KEY = os.getenv("DJANGO_SECRET_KEY", "default-safe-env-secret-key-1234567890")
ALLOWED_HOSTS = ["api.myproduction.com"]

SESSION_COOKIE_SECURE = True
CSRF_COOKIE_SECURE = True
