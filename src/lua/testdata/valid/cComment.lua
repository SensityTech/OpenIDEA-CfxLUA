/* a C style comment */
local a = 1 /* inline */ + 2
/*
  multi-line
  comment
*/
local b = a // 2 -- integer division, not a comment
local c = a /* nested /* is not nested */ * 3
--/* just a line comment
local d = "/* in a string */"
