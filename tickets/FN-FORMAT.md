Reply format for edits to LARGE files (do not resend whole large files):
For each item you change, output the COMPLETE new item (with its doc comments and attributes):

=== FN: <repo-relative path> :: <item name> ===
<complete fn / struct / enum / const / impl block named exactly <item name>>
=== END FN ===

The lead replaces the existing item of that name (located by `fn <name>`, `struct <name>`, `impl <name>`, ...).
For NEW items or new test modules use:

=== APPEND: <repo-relative path> ===
<new items>
=== END APPEND ===

Whole NEW files still use `=== FILE: path ===` ... `=== END FILE ===`. Then the usual NOTES block.
Keep reasoning short and spend the reply on the edits.
