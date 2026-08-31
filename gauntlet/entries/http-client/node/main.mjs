const baseUrl = process.argv[2] ?? "http://127.0.0.1:18400";

async function request(path) {
  const response = await fetch(new URL(path, baseUrl));
  let body = null;
  try {
    body = await response.json();
  } catch {
    // The peer's error body is intentionally ignored for missing items.
  }
  return { status: response.status, body };
}

const listing = await request("/items");
if (listing.status !== 200) throw new Error(`items status ${listing.status}`);
const items = listing.body.items;
let totalQuantity = 0;
for (const itemId of [2, 5, 99]) {
  const result = await request(`/items/${itemId}`);
  if (result.status === 404) {
    console.log(`item ${itemId} missing`);
  } else {
    console.log(`item ${result.body.id} ${result.body.name} qty=${result.body.qty}`);
  }
}
console.log(`total-qty ${items.reduce((sum, item) => sum + item.qty, 0)}`);
