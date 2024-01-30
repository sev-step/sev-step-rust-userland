#!/bin/bash

nasm -f elf64 victim_function.asm -o victim_function.o
clang main.c ../parse_pagemap.c victim_function.o -falign-functions=4096 -O0 -lreadline