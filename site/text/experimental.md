## Inline Macros

Following an inline function's `()`s with one or more `!`s will make it an inline index macro.
This allows `^` placeholders to be used inside the function.

```uiua
# Experimental!
(^0^0)!↯ 2 3 4
```

```uiua
# Experimental!
StdDev ← √(^0^1^0)‼(÷⧻⟜/+|×.-).
StdDev [1 2 3 4]
```

An inline code macro can be specified by putting a `^` between the `)` and the first `!`.

```uiua
# Experimental!
(⇌)^‼(⊂1|⊂2) []
```

```uiua
# Experimental!
($"_ ← 5"⊢)^!X
X
```

```uiua
# Experimental!
(⋅⊢)^!+
(⋅⊢)^!⊓+¯
```

## [derivative](/docs/derivative) and [integral](/docs/integral)

These modifiers transform a mathematical expression.

Currently, only polynomials are supported.

```uiua
# Experimental!
∂(×.) 5                 # x² → 2x
∂√ 1/9                  # √x → 1/(2√x)
∂(-4+⊃(ⁿ2|×¯2)) [0 1 2] # x² - 2x - 4  →  2x² - 2x
```

```uiua
# Experimental!
∫(×.) 3   # x² → x³/3
∫√ 1      # √x → (2x^1.5)/3
∫(+5×2) 2 # 2x + 5  →  x² + 5x
```

## Data Definitions

Data definitions allow you to define structured data whose fields can be accessed by name.

The most basic way to define a data definition is with a `~` followed by a name and some field names inside stack array syntax.

```uiua
# Experimental!
~MyData {Foo Bar}
```

This generates a [module](/tutorial/modules) with a constructor as well as field accessors for the given names.

The constructor has the name `New`, which allows it to be called with the module's name.

```uiua
# Experimental!
~MyData {Foo Bar}
MyData "wow!" 5
MyData~Bar .
```

Notice that the created structure is just a normal box array. The values of the fields are [label](/tutorial/codetactility#labels)led with their name.

The field accessors both [un](/docs/un)[box](/docs/box) and un-label the retrieved values.

If `[]`s are used instead of `{}`s, the fields will not be boxed or labelled.

```uiua
# Experimental!
~Color [r g b a]
Color 1 0.5 0 1
```

The field accessors can be used with [under](/docs/under) modify or replace the value.

```uiua
# Experimental!
~MyData {Foo Bar}
MyData "wow" 5
⍜MyData~Bar(+1) .
⍜MyData~Foo⋅"cool"
```

The [un](/docs/un)[by](/docs/by) idiom also allows you to easily set a value.

```uiua
# Experimental!
~MyData {Foo Bar}
MyData "wow" 5
°⊸MyData~Foo "cool"
```

You can set an initial value for a field by writing it like a binding.

```uiua
# Experimental!
~MyData {Foo Bar ← 0}
MyData 5
```

The initializer can be a function. This will pre-process the value before construction.

Multiple initialized fields can be separated by newlines or `|`s.

```uiua
# Experimental!
~MyData {Foo ← ⊂5⇌|Bar ← 0}
MyData 1_2_3
```

You can also add validation functions to a field. This function will be called both upon construction (after the initializer) and upon mutation.

The function should come after the name and a `:`, but before the initializer.

A common use case for this is to validate the type of a field.

```uiua should fail
# Experimental!
~MyData {Foo: °0type|Bar: °1type}
MyData 1 "hi" # Works
MyData 3 5    # Fails
```

```uiua should fail
# Experimental!
~MyData {Foo: °0type|Bar: °1type}
MyData 1 "hi"
°⊸MyData~Bar 5
```

You can put a data definition inside a scoped module if you'd like to define other functions that use the data. If the name is omitted, the name of the module will be used.

```uiua
# Experimental!
┌─╴MyData
  ~{Foo Bar}
  Format ← /$"Foo is _ and Bar is _"
└─╴
MyData~Format MyData 1_2_3 5
```

If instead of a `~`, you use a `|` followed by a name, the data definition will be treated as a *variant* of the enclosing module.

The constructors for these variants will prepend a tag to the data so that they can be disambiguated. The field accessors will skip the tag.

Because the constructed variants are tagged with incrementing integers, they can be [pattern-matched](/tutorial/patternmatching) on, perhaps in [try](/docs/try).

Variants may be empty.

```uiua
# Experimental!
┌─╴M
  |Foo {Bar Baz}
  |Qux [x y z]
  |Sed {M N}
  |Wir
  
  Format ← ⍣(
    $"_ and _" °Foo
  | $"⟨_ _ _⟩" °Qux
  | $"_: _" °Sed
  | "Wir!" °Wir
  )
└─╴
M~Format M~Foo 2 5
M~Format M~Qux 0 4 1
M~Format M~Sed "Name" "Dan"
M~Format M~Wir
```

A data definition's name can be used as a monadic macro. The field getters will be in scope inside the macro.

```uiua
# Experimental!
~MyData {Foo Bar}
MyData!(+⊃Foo Bar New) 3 5
```

A `Fields` item is generated which contains the field names as boxed strings.
```uiua
# Experimental!
~Foo {Bar Baz Qux}
Foo~Fields
```

If some code immediately follows the data definition, a `Call` function will be generated in which the field names will be mapped to the arguments.

This is called a *data function* and essentially allows for named function arguments.

```uiua
# Experimental!
~MyData {Foo Bar} ↯2 Foo_Foo_Bar
MyData 3 5
```

You can mix and match accessed fields and normal function inputs. Values at the top of the stack will be bound first.

```uiua
# Experimental!
~Foo [x] -x
Foo 3 5
```

```uiua
# Experimental!
~Quad [a b c] ÷×2a -b ⊟¯.√ℂ0 -/×4_a_c ×.b
Quad 1 ¯3 2
```

Note that in general, functions should not be written this way. Keeping an array as local value means it will be duplicated if it is mutated, which is inefficient.

Data functions are mainly useful when your function has a lot of configuration parameters. Arrays that are the primary thing being transformed, as well as arrays that are potentially large, should be kept on the stack.

This concept can be extended to *methods*. Methods are specified within a module that has a data definition already defined. The method is defined in the same way as a normal function, but with a `~` before the name.

When a method is called, a data array is bound as a sort of local variable. Refering to the data definition's fields will pull them from the bound array.

```uiua
# Experimental!
┌─╴Foo
  ~{Bar Baz}
  ~Sum ← +Bar Baz
└─╴
Foo~Sum Foo 3 5
```

Within the body of a method, the bound array can be updated with [un](/docs/un) or [under](/docs/under). The entire bound array can be retrieved via an implicit `Self` binding. The bound array is not returned from the method by default, so `Self` can be used to retrieve it.

Note that the array to be bound in the method is passed *below* any additional arguments. So in the example below, `10` is passed to `AddToBar` *above* the `Foo` array.

```uiua
# Experimental!
┌─╴Foo
  ~{Bar Baz}
  ~Sum      ← +Bar Baz
  ~AddToBar ← Self ⍜Bar+
└─╴
Foo~AddToBar 10 Foo 3 5
Foo~Sum .
```

If one method is referenced from another, it will access the same bound array.

```uiua
# Experimental!
┌─╴Foo
  ~{Bar Baz}
  ~AddBar ← +Bar
  ~Add    ← AddBar Baz
└─╴
Foo~Add Foo 3 5
```

If you want to access the normal getter function for a field, instead of the local-retrieving one, you disambiguate with the name of the module.

```uiua
# Experimental!
┌─╴Foo
  ~{Bar Baz}
  # Demonstrative. Don't do this.
  ~Add ← Foo ⊃(+Bar Foo~Bar|+Baz Foo~Baz)
└─╴
Foo~Add Foo 20 10 Foo 3 5
```