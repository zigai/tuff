match value:
    case {"kind": "user", "name": name}:
        print(name)
    case [first, *rest]:
        print(first, rest)
    case _:
        pass
