class Box[T]:
    def __init__(self, value: T):
        self.value = value

def identity[T](value: T) -> T:
    return value
