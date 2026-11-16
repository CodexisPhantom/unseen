// Check 2: sign-up and sign-in through the official Svelte client (better-auth@1.7.6).
// Node has no cookie jar, so the session cookie is carried by hand.
import assert from "node:assert/strict";
import { createAuthClient } from "better-auth/svelte";

const origin = "http://localhost:7979";
let cookie = "";
const client = createAuthClient({
  baseURL: origin,
  fetchOptions: {
    onRequest(ctx) {
      ctx.headers.set("origin", origin);
      if (cookie) ctx.headers.set("cookie", cookie);
    },
    onResponse(ctx) {
      const set = ctx.response.headers.getSetCookie();
      if (set.length) cookie = set.map((c) => c.split(";")[0]).join("; ");
    },
  },
});

const user = { name: "Owner", email: "owner@example.com", password: "password1234" };

let r = await client.signUp.email(user);
assert.equal(r.error, null, JSON.stringify(r.error));
assert.equal(r.data.user.email, user.email);

r = await client.getSession();
assert.equal(r.data?.user.email, user.email, "session after sign-up");

r = await client.signOut();
assert.equal(r.error, null, JSON.stringify(r.error));
r = await client.getSession();
assert.equal(r.data, null, "no session after sign-out");

r = await client.signIn.email({ email: user.email, password: "wrong-password" });
assert.equal(r.error?.status, 401, "wrong password is refused");

r = await client.signIn.email({ email: user.email, password: user.password });
assert.equal(r.error, null, JSON.stringify(r.error));
r = await client.getSession();
assert.equal(r.data?.user.email, user.email, "session after sign-in");

// Check 3: sign-up is closed after the first user.
r = await client.signUp.email({ ...user, email: "second@example.com" });
assert.equal(r.error?.status, 403);
assert.equal(r.error?.code, "signup_closed");

console.log("client checks passed");
