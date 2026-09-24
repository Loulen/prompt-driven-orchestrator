const PRODUCTS = [
  { id: "mug", name: "Stoneware mug", price: 1800 },
  { id: "candle", name: "Beeswax candle", price: 1250 },
  { id: "throw", name: "Wool throw", price: 8900 },
  { id: "kettle", name: "Enamel kettle", price: 4200 },
];

const cart = new Map();

function formatPrice(cents) {
  return `$${(cents / 100).toFixed(2)}`;
}

function renderProducts() {
  const list = document.getElementById("products");
  list.innerHTML = "";
  for (const product of PRODUCTS) {
    const item = document.createElement("li");
    item.className = "card";
    item.innerHTML = `<h3>${product.name}</h3><p>${formatPrice(product.price)}</p>`;
    const add = document.createElement("button");
    add.type = "button";
    add.textContent = "Add to cart";
    add.addEventListener("click", () => addToCart(product.id));
    item.append(add);
    list.append(item);
  }
}

function addToCart(id) {
  cart.set(id, (cart.get(id) ?? 0) + 1);
  renderCart();
}

function renderCart() {
  let count = 0;
  let total = 0;
  const lines = document.getElementById("cart-lines");
  lines.innerHTML = "";
  for (const [id, quantity] of cart) {
    const product = PRODUCTS.find((p) => p.id === id);
    count += quantity;
    total += product.price * quantity;
    const line = document.createElement("li");
    line.textContent = `${quantity} × ${product.name}`;
    lines.append(line);
  }
  document.getElementById("cart-count").textContent = String(count);
  document.getElementById("cart-total").textContent = formatPrice(total);
}

document.getElementById("cart-button").addEventListener("click", () => {
  const panel = document.getElementById("cart");
  panel.hidden = !panel.hidden;
});

renderProducts();
renderCart();
