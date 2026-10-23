(function_declaration) @function.around
(function_declaration body: (block) @function.inside)
(type_declaration) @class.around
(type_declaration) @class.inside
(line_comment)+ @comment.around
(block_comment) @comment.around
