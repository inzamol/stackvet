# b.py imports a (creates cycle a -> b -> a: IMP-001)
from .a import func_a

def helper_b():
    return "b"
