from django.http import HttpResponse

def regular_view(request):
    return HttpResponse("Clean response")
