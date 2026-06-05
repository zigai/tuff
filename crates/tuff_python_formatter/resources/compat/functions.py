def create_user(
    id: int,
    display_name: str,
    active: bool=True,
) -> User:
    return User(id=id, display_name=display_name, active=active)
