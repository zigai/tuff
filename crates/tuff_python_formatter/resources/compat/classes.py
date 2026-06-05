class User:
    id: int
    display_name: str
    active: bool = True

    def greet(self)->str:
        return f"hello {self.display_name}"
