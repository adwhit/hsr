
Strategy: Each API path names a type.

When we try to construct them, we might find that they are concrete types that
need to be created

e.g. for path
ApiPath(["paths", "/pets/{id}", "DELETE", "reponses", "default", "application/json"])
=>
struct PathPetIdDeleteResponseDefaultJson {
  sucess: bool
}
  
Or we might be able to simply alias the type to a known type

// built-in
type PathPetIdDeleteResponseDefaultJson = String

// generated elsewhere
type PathPetIdDeleteResponseDefaultJson = NewPet

in any case, we always use the full name when creating our endpoint

fn delete_pet() -> PathPetIdDeleteResponseDefaultJson;

That way, we don't have to worry about chasing references around the place
