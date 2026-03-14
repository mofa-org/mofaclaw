import re, sys

filepath = sys.argv[1]
with open(filepath, "r", encoding="utf-8") as f:
    content = f.read()

# Test the exact patterns from comp_modifier._rewrite_all_fonts
print("=== Inline Font Matches ===")
pat1 = re.compile(r'(\bFont\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"')
for m in pat1.finditer(content):
    print(f"  Found: Font = \"{m.group(2)}\" at position {m.start()}")

print("\n=== Inline Style Matches ===")
pat2 = re.compile(r'(\bStyle\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"')
for m in pat2.finditer(content):
    print(f"  Found: Style = \"{m.group(2)}\" at position {m.start()}")

print("\n=== Bracket Font Matches ===")
pat3 = re.compile(r'(\["Font"\]\s*=\s*Input\s*\{\s*Value\s*=\s*)"([^"]*)"')
for m in pat3.finditer(content):
    print(f"  Found: [\"Font\"] = \"{m.group(2)}\" at position {m.start()}")

print("\n=== Font Spline Matches ===")
pat4 = re.compile(r'(\w+Font)\s*=\s*BezierSpline\s*\{', re.MULTILINE)
for m in pat4.finditer(content):
    print(f"  Found spline: {m.group(1)}")

# Test replacement
print("\n=== Simulated Replacement ===")
test_content = content
new_content, count = pat1.subn(r'\g<1>"Arial"', test_content)
print(f"  Plain Font replacements: {count}")
new_content2, count2 = pat2.subn(r'\g<1>"Regular"', new_content)
print(f"  Plain Style replacements: {count2}")