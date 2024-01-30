#include <stdio.h>
#include <string.h>
//requires libreadline-dev on ubuntu. compile with -lreadline
#include <readline/readline.h>
#include <readline/history.h>

int main(int argc, char** argv) {

    printf("some unrelated output\n");
    printf("VMSERVER::VAR var_1 value_var_1\n");
    printf("VMSERVER::VAR var_2 value_var_2\n");

    printf("VMSERVER::SETUP_DONE\n");

    printf("Waiting for \"VMSERVER::START\" on stdin\n");
    while(1) {
        char* in = readline(NULL);

        if(0 == strcmp(in, "VMSERVER::START")) {
            break;
        }
    }

    printf("Got start signal\n");

}