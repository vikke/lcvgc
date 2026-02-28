if exists('g:loaded_lcvgc') | finish | endif
let g:loaded_lcvgc = 1

lua require('lcvgc').register_commands()
