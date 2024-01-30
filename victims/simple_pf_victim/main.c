#include <stdio.h>
#include <string.h>
#include <stdlib.h>

//requires libreadline-dev on ubuntu. compile with -lreadline
#include <readline/readline.h>
#include <readline/history.h>

#include <sys/mman.h>
#include <unistd.h>

#include "../parse_pagemap.h"

__attribute__((noinline))
void marker_fn1() {
    printf("Marker function 1 called\n");
}

__attribute__((noinline))
void marker_fn2() {
    printf("Marker function 2 called\n");
}

__attribute__((noinline))
void marker_fn3() {
    printf("Marker function 3 called\n");
}

extern void victim_fn(uint64_t* v);

typedef struct {
    char* name;
    uint64_t vaddr;
} code_gadget_t;

int main(int argc, char** argv) {

    uint64_t* mem_buffer = (uint64_t*)mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_POPULATE, -1, 0);
    if (mem_buffer == MAP_FAILED) {
        perror("mmap");
        exit(EXIT_FAILURE);
    }

    //make sure everything is faulted into memory
    marker_fn1();
    marker_fn2();
    victim_fn(mem_buffer);
    marker_fn3();

    //print interesting code locations
    code_gadget_t gadgets[] = {
        {
            .name = "marker_fn1",
            .vaddr = (uint64_t)marker_fn1,
        },
        {
            .name = "marker_fn2",
            .vaddr = (uint64_t)marker_fn2,
        },
        {
            .name = "marker_fn3",
            .vaddr = (uint64_t)marker_fn3,
        },
        {
            .name = "victim_fn",
            .vaddr = (uint64_t)victim_fn,
        },
        {
            .name = "mem_buffer",
            .vaddr = (uint64_t)mem_buffer,
        },

    };

    pid_t pid = getpid();
    for(size_t i = 0; i < sizeof(gadgets)/sizeof(gadgets[0]); i++) {
        code_gadget_t* g = gadgets+i;
        uint64_t paddr;
        if(virt_to_phys_user(&paddr, pid, g->vaddr)) {
            printf("Failed to translate vaddr 0x%jx of gadget %s to paddr\n", g->vaddr, g->name);
            return -1;
        }
        printf("VMSERVER::VAR %s 0x%jx\n", g->name, paddr);
        printf("VMSERVER::VAR %s_vaddr 0x%jx\n", g->name, g->vaddr);

    }




    printf("VMSERVER::SETUP_DONE\n");

    printf("Waiting for \"VMSERVER::START\" on stdin\n");
    while(1) {
        char* in = readline(NULL);

        if(0 == strcmp(in, "VMSERVER::START")) {
            break;
        }
    }


    marker_fn1();
    marker_fn2();
    victim_fn(mem_buffer);
    marker_fn3();
    
    

}