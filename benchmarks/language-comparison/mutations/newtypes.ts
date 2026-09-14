type UserId = number & { readonly kind: "UserId" };
type ProductId = number & { readonly kind: "ProductId" };
function load(user: UserId): number { return user; }
const product = 1 as ProductId;
console.log(load(product));
