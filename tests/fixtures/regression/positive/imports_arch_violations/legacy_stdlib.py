# IMP-004: Module 'cgi' was removed in Python 3.13 (PEP 594)
import cgi

def parse(data):
    return cgi.parse(data)
