from django.views.decorators.csrf import csrf_exempt
from django.http import HttpResponse

# DJG-006: @csrf_exempt applied to view
@csrf_exempt
def webhook_view(request):
    return HttpResponse("ok")
