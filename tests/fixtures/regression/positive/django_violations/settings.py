# Django settings with intentional violations

# DJG-001: Hardcoded DEBUG = True
DEBUG = True

# DJG-002: Hardcoded django-insecure secret key
SECRET_KEY = 'django-insecure-hardcoded-secret'

# DJG-003: Wildcard ALLOWED_HOSTS
ALLOWED_HOSTS = ['*']

# DJG-004: Insecure cookie settings
SESSION_COOKIE_SECURE = False
CSRF_COOKIE_SECURE = False
